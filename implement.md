# 概要

Filesystem in Userspace(FUSE) はユーザ空間でファイルシステムを実現する仕組みである。

一般的にファイルシステムを作るというと、カーネルモジュールを作成しなければならないので、いろいろと苦労が多い。  
FUSEを使えば大分楽に実装できる。また、HDDなどの実デバイスに直接読み書きするだけでなく、仮想的なファイルシステムを作るのにも都合がよい。

そんな訳で、FUSEを使ったSSH as a filesystem や AWS S3 as a filesystemといった
「読み書きできる何かをファイルシステムとしてマウント出来るようにするソフトウェア」があれこれと存在する。

ただし、カーネルモジュールを作るより楽とはいえ、FUSEを使ったソフトウェアの作成には困難が伴う。  
ある程度ファイルシステムの知識は必要だし、チュートリアルはほどほどの所で終わってしまい、「あとはsshfsの実装などを見てくれ！」とコードの海に投げ出される。

本書は、RustによるFUSEインターフェースの実装である `rust-fuse` を用いてFUSEを使ったファイルシステムの実装に挑戦し、
気をつけるべき点などを記録したものである。

## FUSEの仕組み(アバウト)

FUSE本体はLinuxカーネルに付属するカーネルモジュールであり、大抵のディストリビューションではデフォルトでビルドされてインストールされる。

FUSEがマウントされたディレクトリ内のパスに対してシステムコールが呼ばれると、以下のように情報がやりとりされる。

```
システムコール <-> VFS <-> FUSE <-> FUSEインターフェース <-> 自分のプログラム
```

詳しくは [Wikipediaの図](https://ja.wikipedia.org/wiki/Filesystem_in_Userspace) を見てほしい。

## FUSEインターフェース

FUSEはデバイス `/dev/fuse` を持ち、ここを通じてユーザ空間とやりとりを行う。  
前項の `FUSE <-> FUSEインターフェース` の部分である。

規定のプロトコルを用いて `/dev/fuse` にデータを渡したり受け取ったりするのがFUSEインターフェースである。  
有名な実装として、 [libfuse](https://github.com/libfuse/libfuse) がある。  
このlibfuseが大変強力なので、大抵の言語でのFUSEインターフェースはlibfuseのラッパーになっている。

## rust-fuse
Rustには独自の実装 `Rust FUSE(rust-fuse)` がある。ありがたいですね。  
元々プロトコルが同じなので、インターフェースの関数はlibfuseと大変似ている。そのため、libfuseの知見が使える。ありがたいですね。

現時点(2019/10) の最新版は0.3.1で、2年ぐらい更新されていないが、次バージョン(0.4.0)が開発中である。

# データの保存先
今回はデータの保存先にsqliteを使用する。  
sqliteは可変長のバイナリデータを持てるので、そこにデータを書き込む。
メタデータの読み書きも割と簡単にできるだろう。

## データベース構造
テーブルはメタデータテーブル(MDT)とディレクトリエントリテーブル(DET)とブロックデータテーブル(BDT)3つに分ける。  
今後拡張ファイル属性が必要になってきた場合、拡張属性データテーブル(XATTRT)を追加する。

## MDT
メタデータは一般的なファイルシステムのメタデータと同様で、fuseが必要なデータを持つ。  
rust-fuseのメタデータ構造体は以下のようになっている。

```
pub struct FileAttr {
    /// Inode number
    pub ino: u64,
    /// Size in bytes
    pub size: u64,
    /// Size in blocks Sparse File に対応する場合、実際に使用しているブロック数を返す
    pub blocks: u64,
    /// Time of last access read(2)実行時に更新される
    pub atime: SystemTime,
    /// Time of last modification write(2)またはtruncate(2)実行時に更新される
    pub mtime: SystemTime,
    /// Time of last change メタデータ変更時に更新される。 write(2)またはtruncate(2)でファイル内容が変わるときも更新される
    pub ctime: SystemTime,
    /// Time of creation (macOS only)
    pub crtime: SystemTime,
    /// Kind of file (directory, file, pipe, etc)
    pub kind: FileType,
    /// Permissions
    pub perm: u16,
    /// Number of hard links
    pub nlink: u32,
    /// User id
    pub uid: u32,
    /// Group id
    pub gid: u32,
    /// Rdev
    pub rdev: u32,
    /// Flags (macOS only, see chflags(2))
    pub flags: u32,
}
```

以下のようなテーブルを作る。

|列名 | 型 | 概要|
|---|---|---|
|id|integer primary|ファイルのinode番号 (pkey)|
|size|int|ファイルサイズ|
|atime|text|アクセス時刻|
|atime_nsec|int|アクセス時刻(小数点以下)|
|mtime|text|修正時刻|
|mtime_nsec|int|修正時刻(小数点以下)|
|ctime|text|ステータス変更時刻|
|ctime_nsec|int|ステータス変更時刻(小数点以下)|
|crtime|text|作成時刻(mac用)|
|crtime_nsec|int|作成時刻(小数点以下)|
|mode|int|パーミッション(ファイル種別含む)|
|nlink|int|ハードリンク数|
|uid|int|uid|
|gid|int|gid|
|rdev|int|デバイスタイプ|
|flags|int|フラグ(mac用)|

idをinteger primary keyにする。これをinode番号とする。  
FUSEはシステムコールの `stat(2)` の `st_mode` と同様に、modeにファイル種別を入れて渡してくるので、permとkindはmodeから得る。
`libc::S_IFREG` 等でANDを取るとどの種類かが分かる。

## BDT
ブロックデータテーブルのblobにデータを格納する。
BDTはファイルのinode, 何番目のブロックか、の列を持つ

|列名 | 型 | 概要|
|---|---|---|
|file_id|int|ファイルのinode番号 (pkey)(foreign key)|
|block_num|int|データのブロック番号(1始まり)(pkey)|
|data|blob|データ(4kByte単位とする)|

外部キー `foreign key (file_id) references metadata(id) on delete cascade`
を指定する事で、ファイルのメタデータが消えたらデータも削除されるようにする。

主キーとして `(file_id, block_num)` を指定する。

## DET
ディレクトリ構造を表現する方法は、以下の2つの候補がある

1. 分散ファイルシステムでよくある、フルパスを各ファイルが持っていて、文字列操作で各ディレクトリの情報を得る方法
1. 一般的なファイルシステムのように、ディレクトリエントリを作る方法

今回は実装の楽そうな後者のディレクトリエントリ方式で行う。

必要そうなのは以下のデータ

|列名 | 型 | 概要|
|---|---|---|
|parent_id|int|親ディレクトリのinode (pkey)(foreign key)|
|child_id|int|子ファイル/子ディレクトリのinode (foreign key)|
|file_type|int|ファイルタイプ|
|name|text|ファイル/ディレクトリ名 (pkey)|

あらゆるディレクトリは `.` と `..` のエントリを持つ

# Hello!
## 概要
第一段階として、rust-fuseに付属する、サンプルプログラムの `HelloFS` と同じ機能を実装する。
`HelloFS` は以下の機能がある。

1. ファイルシステムはリードオンリー
1. ルート直下に `hello.txt` というファイルがあり、 `"Hello World!\n"` というデータが読み込める

必要なのは以下の4つの関数である。

```
fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry){
    ...
}
fn getattr(&mut self, _req: &Request, ino: u64, reply: ReplyAttr) {
    ...
}
fn read(&mut self, _req: &Request, ino: u64, _fh: u64, offset: i64, _size: u32, reply: ReplyData) {
    ...
}
fn readdir(&mut self, _req: &Request, ino: u64, _fh: u64, offset: i64, mut reply: ReplyDirectory) {
    ...
}
```



## DB関数実装
データベースを読み書きする関数を実装していく。  
今回必要な関数は以下の通り。

```
pub trait DbModule {
    /// ファイルのメタデータを取得する。見つからない場合は0を返す
    fn get_inode(&self, inode: u32) -> Result<DBFileAttr, SqError>;
    /// ディレクトリのinodeを指定して、ディレクトが持つディレクトリエントリを全て取得する
    fn get_dentry(&self, inode: u32) -> Result<Vec<DEntry>, SqError>;
    /// 親ディレクトリのinodeと名前から、ファイルやサブディレクトリのinodeとメタデータを得る
    fn lookup(&self, parent: u32, name: &str) -> Result<DBFileAttr, SqError>;
    /// inodeとブロック数を指定して、1ブロック分のデータを読み込む
    fn get_data(&self, inode: u32, block: u32, length: u32) -> Result<Vec<u8>, SqError>;
    /// DBのブロックサイズとして使っている値を得る
    fn get_db_block_size(&self) -> u32;
}
```
