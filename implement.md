# 概要

## 1行で
この記事は、RustによるFUSEインターフェースの実装である `fuse-rs` を用いてFUSEを使ったファイルシステムの実装に挑戦し、
得られた知見などを記録したものです。

## 概要

Filesystem in Userspace(FUSE) はLinuxのユーザ空間でファイルシステムを実現する仕組みです。

一般的にファイルシステムを作るというと、カーネルモジュールを作成しなければならないのでいろいろと苦労が多いですが、FUSEを使えば比較的楽に実装できます。  
また、HDDなどの実デバイスに直接読み書きするだけでなく、ネットワークストレージを利用した仮想的なファイルシステムを作るのにも都合がよいです。

そんな訳で、FUSEを使ったSSH as a filesystem や AWS S3 as a filesystemといった
「読み書きできる何かをファイルシステムとしてマウント出来るようにするソフトウェア」があれこれと存在します。  
上記のようなソフトウェアの代表例である `sshfs` や `s3fs` は使った事のある人もいるのではないでしょうか。

元々はLinuxの一部でしたが、MacOSやFreeBSDでも使用できます。最近ではWindowsのWSL2でも使えるようになるようです。WSLの導入の手間が要るとはいえ、WindowsでもFUSEが使えるのは嬉しいですね。  
ちなみにWindowsには、Fuseに似た仮想ファイルシステムである [Dokan](https://github.com/dokan-dev/dokany) もあります。

ただし、カーネルモジュールを作るより楽とはいえ、FUSEを使ったソフトウェアを作成するのは大変です。  
ある程度ファイルシステムの知識は必要ですし、何か調べようとしてもドキュメントが少なく、
チュートリアルを見てもほどほどの所で終わってしまい、「あとはサンプルとsshfsの実装などを見てくれ！」とコードの海に投げ出されます。

そこで、各所の情報をまとめつつ、自分で0からファイルシステムを実装して気をつける点などを見つけていきます。

## 参考資料
[Rust FUSE](https://github.com/zargony/fuse-rs) : Rust版Fuseインターフェースのプロジェクト  
[libfuse](https://github.com/libfuse/libfuse) : C版のFuseインターフェースライブラリ  
[osxfuse](https://github.com/osxfuse/fuse) : MacOS向けのFuseインターフェースライブラリ  
[FUSEプロトコルの説明](https://john-millikin.com/the-fuse-protocol) : カーネルモジュール <-> Fuseライブラリ間のプロトコル  
[VFSの説明](https://ja.osdn.net/projects/linuxjf/wiki/vfs.txt)  
[fuse_lowlevel.h(libfuseのヘッダ)](https://github.com/libfuse/libfuse/blob/master/include/fuse_lowlevel.h): lowlevel関数の説明  
[fuse_common.h(libfuseのヘッダ)](https://github.com/libfuse/libfuse/blob/master/include/fuse_common.h)  
[Linuxプログラミングインターフェース(書籍)](https://www.oreilly.co.jp/books/9784873115856/) : システムコールがどう動くべきかは大体ここを見て判断する  
[libfuseのメーリングリストのアーカイブ](https://sourceforge.net/p/fuse/mailman/fuse-devel/)  
[gcsf](https://github.com/harababurel/gcsf) : fuse-rsを使ったファイルシステムの例  

## 実験環境
プログラムは全て次の環境で実験しています。

Linux: 5.3.11
ディストリビューション: Fedora 31
Rust: 1.39.0
fuse-rs: 0.3.1

## FUSEの仕組み(概要)

FUSE本体はLinuxカーネルに付属するカーネルモジュールで、大抵のディストリビューションではデフォルトで有効になっています。

FUSEを使ったファイルシステムがマウントされたディレクトリ内に対してシステムコールが呼ばれると、以下のように情報がやりとりされます。

```
システムコール <-> VFS <-> FUSE <-(FUSE ABI)-> FUSEインターフェース <-(FUSE API)-> 自作のファイルシステム <-> デバイスやネットワーク上のストレージ
```

[Wikipediaの図](https://ja.wikipedia.org/wiki/Filesystem_in_Userspace) を見ると分かりやすいです。  
本来であればVFSの先に各ファイルシステムのカーネルモジュールがあるのですが、FUSEは受け取った情報をユーザ空間に横流ししてくれます。

## FUSEインターフェース

FUSEはデバイス `/dev/fuse` を持ち、ここを通じてユーザ空間とやりとりを行います。  
前項の `FUSE <-> FUSEインターフェース` の部分です。

規定のプロトコル(FUSE ABI)を用いて `/dev/fuse` に対してデータを渡したり受け取ったりするのがFUSEインターフェースです。  
有名なライブラリとして、C/C++用の [libfuse](https://github.com/libfuse/libfuse) があります。  
このlibfuseが大変強力なので、大抵の言語でのFUSEインターフェースはlibfuseのラッパーになっています。

libfuseを使うと、 `open`, `read`, `write` 等の関数を決められた仕様通りに作成して登録するだけで、ファイルシステムとして動作するようになっています。
例えば、 `read(2)` のシステムコールが呼ばれると、最終的に自作のファイルシステムの `read` 関数が呼ばれます。  

```c
// read関数を、常にランダムな内容を返すようにした例
int my_read(const char *path, char *buf, size_t size, off_t offset, struct fuse_file_info *fi) {
    ssize_t res;
    res = getrandom(buf, size, 0);
    return (int)res;
}
```

登録すべき関数は、 `fuse.h` 内で定義されている通常のものと、 `fuse_lowlevel.h` 内で定義されている低級なものがあります。  
ファイルシステムを作成する場合、どちらの関数群を実装するか選択する必要があります。  
`fuse.h` の方はおおよそシステムコールと1:1で対応しています。 `lowlevel` の方は `FUSE ABI` と1:1になるように作られています。

## fuse-rs
Rustには(ほぼ)独自のFUSEインターフェースの実装 [Rust FUSE(fuse-rs)](https://github.com/zargony/fuse-rs) があります。ありがたいですね。  
プロトコルが同じなので、インターフェースの関数(FUSE API)はlibfuseのlowlevel関数と大変似ています。
そのため、何か困った時にはlibfuseの情報が流用できたりします。  

現時点(2019/10) の最新版は0.3.1で、2年ぐらい更新されていませんが、次バージョン(0.4.0)が開発中です。  
0.3.1と0.4.0では仕様が大きく異なるので注意してください。  
また、0.3.1では対応するプロトコルのバージョンが7.8で、最新のものと比較していくつかの機能がありません。

libfuseはマルチスレッドで動作し、並列I/Oに対応していますが、fuse-rsはシングルスレッドのようです。

使用するためには、 `Cargo.toml` に以下のように記述します。

```toml
[dependencies]
fuse = "0.3.1"
```

# データの保存先
今回自分でファイルシステムを実装していく上で、HDDの代わりになるデータの保存先としてsqliteを使用します。  
ライブラリは [rusqlite](https://github.com/jgallagher/rusqlite) を使用します。  
FUSEの実装方法について調べるのがメインなので、こちらについてはざっくりとしか説明しませんが、ご容赦ください。

sqliteは可変長のバイナリデータを持てるので、そこにデータを書き込みます。
トランザクションがあるので、ある程度アトミックな操作ができます。
DBなので、メタデータの読み書きも割と簡単にできるでしょう。

fuse-rsが扱う整数の大半は `u64` ですが、sqliteはunsignedの64bit intに対応していないので、厳密にやろうとするといろいろと面倒になります。  
とりあえず全部 `u32` にキャストする事にしますが、気になる場合は `i64` にキャストして、大小比較を行うユーザ定義関数をsqlite上に作成したり、
`u32` 2個に分割したりしてください。

DBの構造についてざっくりと説明していきます。

## データベース構造
テーブルはメタデータテーブル(MDT)とディレクトリエントリテーブル(DET)とブロックデータテーブル(BDT)の3つに分けます。  
今後拡張ファイル属性が必要になってきた場合、拡張属性データテーブル(XATTRT)を追加します。

以下では各テーブルについて説明していきます。

### MDT
ファイルのinode番号をキーとして検索するとメタデータが返ってくるような、メタデータ用のテーブルを作ります。  
メタデータは一般的なファイルシステムのメタデータと同じような形式です。  
fuse-rsが関数の引数で渡してきたり、戻り値として要求したりするメタデータ構造体は以下のように定義されています。

```rust
// fuse::FileAttr
pub struct FileAttr {
    /// inode番号
    pub ino: u64,
    /// ファイルサイズ(バイト単位)
    pub size: u64,
    /// ブロックサイズ *Sparse File に対応する場合、実際に使用しているブロック数を返す
    pub blocks: u64,
    /// Time of last access. *read(2)実行時に更新される
    pub atime: Timespec,
    /// Time of last modification. *write(2)またはtruncate(2)実行時に更新される
    pub mtime: Timespec,
    /// Time of last change. *メタデータ変更時に更新される。 write(2)またはtruncate(2)でファイル内容が変わるときも更新される
    pub ctime: Timespec,
    /// Time of creation (macOS only)
    pub crtime: Timespec,
    /// ファイル種別 (directory, file, pipe, etc)
    pub kind: FileType,
    /// パーミッション
    pub perm: u16,
    /// ハードリンクされている数
    pub nlink: u32,
    /// User id
    pub uid: u32,
    /// Group id
    pub gid: u32,
    /// Rdev *デバイスファイルの場合、デバイスのメジャー番号とマイナー番号が入る
    pub rdev: u32,
    /// Flags (macOS only, see chflags(2)) *非表示などmac特有の属性が入ります。
    pub flags: u32,
}
```

これに合わせて、以下のようなテーブルを作ります。

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
|kind|int|ファイル種別|
|mode|int|パーミッション(ファイル種別含む)|
|nlink|int|ハードリンク数|
|uid|int|uid|
|gid|int|gid|
|rdev|int|デバイスタイプ|
|flags|int|フラグ(mac用)|

idをinteger primary keyにします。これがinode番号になります。

kindはファイル種別です。
通常ファイル・キャラクターデバイス・ブロックデバイス・FIFO・ソケット・ディレクトリ・シンボリックリンク の7種類があります。  
FUSEでは `stat(2)` と同様に、 `mode` にファイル種別のビットも含まれているので、
ビット操作する必要があります。  
cのlibfuseでは `libc::S_IFMT` (該当ビットのマスク) `libc::S_IFREG` (通常ファイルを示すビット) 等を用いて
`if((mode & S_IFMT) == S_IFREG)` のようにして判別する事ができます。  
fuse-rsの場合はメタデータを返す時はenumで定義されたファイル種別を使い、ビット操作はライブラリ側で処理してくれるので、
実際のビットがどうなっているかを気にするケースはあまりありませんが、  
`mknod` の引数で `mode` が生の値で渡ってくるので、 `mknod` を実装する場合は気をつける必要があります。

### BDT
ファイルのinode番号とファイル内のブロック番号を指定するとデータが返ってくるような、ブロックデータテーブルを作成します。  
ブロックデータテーブル(BDT)のblobにデータを格納します。
BDTはファイルのinode番号, 何番目のブロックか、の列を持ちます。具体的には以下のようになります。

|列名 | 型 | 概要|
|---|---|---|
|file_id|int|ファイルのinode番号 (pkey)(foreign key)|
|block_num|int|データのブロック番号(1始まり)(pkey)|
|data|blob|データ(4kByte単位とする)|

外部キー `foreign key (file_id) references metadata(id) on delete cascade`
を指定する事で、ファイルのメタデータが消えたらデータも削除されるようにします。

「あるファイルのあるブロック」は一意なので、主キーとして `(file_id, block_num)` を指定します。

### DET
ディレクトリ構造を表現する方法は、以下の2つの候補があります。

1. オブジェクトストレージのように、各ファイルがフルパスを記憶していて、文字列操作で各ディレクトリの情報を得る方法
1. 一般的なファイルシステムのように、ディレクトリエントリを作る方法

今回はfuse-rsの関数とも相性のいい後者のディレクトリエントリ方式で行います。  
ディレクトリのinode番号を指定すると、ディレクトリ内の全てのエントリ(ファイル名、ファイルタイプ、inode番号のセット)を返すようなテーブルを作成します。

必要なのは以下のデータです。

|列名 | 型 | 概要|
|---|---|---|
|parent_id|int|親ディレクトリのinode番号 (pkey)(foreign key)|
|child_id|int|子ファイル/子ディレクトリのinode番号 (foreign key)|
|file_type|int|子のファイルタイプ|
|name|text|子のファイル/ディレクトリ名 (pkey)|

あらゆるディレクトリは `.` (自分自身)と `..` (親ディレクトリ)のエントリを持ちます。  
ルートの `..` は自分自身を指すようにします。  
`.` と `..` は返さなくともよい事になっていますが、その場合は呼び出し側のプログラムの責任で処理する事になります。

### ルートディレクトリ
初期データとして、ルートディレクトリの情報を入れます。FUSEでは、ルートディレクトリのinode番号は1です。  
ルートディレクトリは必ず存在する必要があります。

# Hello!
## 概要
第一段階として、fuse-rsに付属する、サンプルプログラムの `HelloFS` と同じ機能を実装します。
`HelloFS` は以下の機能があります。

1. ファイルシステムはリードオンリー
1. ルート直下に `hello.txt` というファイルがあり、 `"Hello World!\n"` という文字列が書き込まれている

`fuse::Filesystem` トレイトの関数を実装していきます。  
`HelloFS` の機能を実現するのに必要なのは以下の4つの関数です。

```rust
use fuse::{
    Filesystem,
    ReplyEntry,
    ReplyAttr,
    ReplyData,
    ReplyDirectory,
    Request
};
impl Filesystem for SqliteFs {
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
}
```

ファイルやディレクトリをopen/closeする関数を実装せずにread関数やreaddir関数を実装していますが、
`libfuse` や `fuse-rs` は全ての関数にデフォルトの実装があり、
今回のようにreadonlyで状態を持たないファイルシステムの場合、自分で実装しなくても動作します。  
これらの関数については今後実装する必要が出てきた時に説明します。


## DB関数
データベースを読み書きする関数です。  
今回作成した関数は以下になります。

```rust
pub trait DbModule {
    /// ファイルのメタデータを取得する。見つからない場合は0を返す
    fn get_inode(&self, inode: u32) -> Result<DBFileAttr, Error>;

    /// ディレクトリのinode番号を指定して、ディレクトが持つディレクトリエントリを全て取得する
    fn get_dentry(&self, inode: u32) -> Result<Vec<DEntry>, Error>;

    /// 親ディレクトリのinode番号と名前から、ファイルやサブディレクトリのinode番号とメタデータを得る
    /// inodeが存在しない場合、inode番号が0の空のinodeを返す
    fn lookup(&self, parent: u32, name: &str) -> Result<DBFileAttr, Error>;

    /// inode番号とブロック数を指定して、1ブロック分のデータを読み込む
    /// ブロックデータが存在しない場合は、0(NULL)で埋められたブロックを返す
    fn get_data(&self, inode: u32, block: u32, length: u32) -> Result<Vec<u8>, Error>;

    /// DBのブロックサイズとして使っている値を得る
    fn get_db_block_size(&self) -> u32;
}

// メタデータ構造体
pub struct DBFileAttr {
    pub ino: u32,
    pub size: u32,
    pub blocks: u32,
    pub atime: SystemTime,
    pub mtime: SystemTime,
    pub ctime: SystemTime,
    pub crtime: SystemTime,
    pub kind: FileType,
    pub perm: u16,
    pub nlink: u32,
    pub uid: u32,
    pub gid: u32,
    pub rdev: u32,
    pub flags: u32,
}

// ディレクトリエントリ構造体
pub struct DEntry {
    pub parent_ino: u32,
    pub child_ino: u32,
    pub filename: String,
    pub file_type: FileType,
}
```

## fuseの関数全般の話
### fuseの関数
ファイルシステムなので、関数は基本的に受け身です。システムコールに応じて呼び出されます。  
fuse-rsでは、 `Filesystem` トレイトが定義されているので、必要な関数を適宜実装していきます。

### 引数
どの関数にも `Request` 型の引数 `req` が存在します。  
`req.uid()` で実行プロセスのuidが、 `req.gid()` でgidが、 `req.pid()` でpidが取得できます。

### 戻り値
`init` 以外の各関数に戻り値は存在せず、引数の `reply` を操作して、呼び出し元に値を受け渡します。  
`ReplyEmpty, ReplyData, ReplyAttr` のように、関数に応じて `reply` の型が決まっています。

`reply.ok()` `reply.error(ENOSYS)` `reply.attr(...)` 等 `reply` の型に応じたメソッドを実行します。

エラーの場合、 `libc::ENOSYS` `libc::ENOENT` のような定数を `reply.error()` の引数に指定します。

## lookup
```rust
fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry);
```

引数の `parent` で親ディレクトリのinode番号、 `name` で当該ディレクトリ/ファイルの名前が与えられるので、メタデータを返します。  
lookup実行時には `lookup count` をファイルシステム側で用意して、増やしたりしなければなりませんが、
今回はreadonlyのファイルシステムなので無視します。  
`lookup count` については `unlink` 実装時に説明します。

replyに必要なデータは以下になります。

```
    //正常
    reply.entry(&TTL, &ATTR, &GENERATION);
    エラーの場合
    reply.error(ENOENT);
```

### Replyの引数
`reply.entry()` の3つの引数について説明します。

#### TTL
`time::Timespec` で期間を指定します。  
TTLの間はカーネルは再度問い合わせに来ません。

今回は、以下のような `ONE_SEC` という定数を作って返しています。

```
const ONE_SEC: Timespec = Timespec{
    sec: 1,
    nsec: 0
};
```

#### ATTR
対象のメタデータ。 `fuse::FileAttr` を返します。

#### GENERATION
inodeの世代情報を `u64` で返します。削除されたinodeに別のファイルを割り当てた場合、
前のファイルと違うファイルである事を示すために、generationに別の値を割り当てます。  
ただし、この値をチェックするのは(知られているものでは)nfsしかありません。  
今回は常時 `0` に設定します。

[libfuseの説明](https://libfuse.github.io/doxygen/structfuse__entry__param.html#a4c673ec62c76f7d63d326407beb1b463)
も参考にしてください。

#### エラー

対象のディレクトリエントリが存在しない場合、 `reply.error(ENOENT)` でエラーを返します。

### 実装
実装は以下のようになります。

```rust
fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
    match self.db.lookup(parent as u32, name.to_str().unwrap()) {
        Ok(attr) => {
            reply.entry(&Timespec{sec: 1, nsec: 0}, &attr.get_file_attr() , 0);
        },
        Err(_err) => reply.error(ENOENT)
    };
}
```

## getattr
```rust
fn getattr(&mut self, _req: &Request, ino: u64, reply: ReplyAttr);
```

引数の `ino` でinode番号が指定されるので、ファイルのメタデータを返します。
メタデータの内容については `lookup` で返す `ATTR` と同じです。

```rust
fn getattr(&mut self, _req: &Request, ino: u64, reply: ReplyAttr) {
    match self.db.get_inode(ino as u32) {
        Ok(attr) => {
            reply.attr(&ONE_SEC, &attr.get_file_attr());
        },
        Err(_err) => reply.error(ENOENT)
    };
}
```

## read
```rust
fn read(&mut self, _req: &Request, ino: u64, _fh: u64, offset: i64, size: u32, reply: ReplyData);
```

引数の `ino` のinode番号で指定されたファイルを、 `offset` で指定されたバイトから `size` で指定されたバイト分読み込みます。  
読み込んだデータは `reply.data(&data)` を実行して返します。

EOFまたはエラーを返す場合を除いて、 `read` 関数は引数の `size` で指定されたサイズのデータを返さないといけません。  
例えば、長さ200byteのファイルに対して、4096byteの要求が来ることがあります。
この場合EOFなので、200byte返す事が許されます。また、200byte以上返しても切り捨てられます。  
それ以外の場合で要求された長さのデータを用意できない場合は、エラーを返さないといけません。

libfuseやfuse-rsの説明では、「もし要求されたサイズより短いサイズのデータを返した場合、0埋めされる」と書いてありますが、
手元の環境では0埋めされず、短いサイズが `read(2)` の結果として返ってきました。  
[カーネルのこのコミット](https://github.com/torvalds/linux/commit/5c5c5e51b26413d50a9efae2ca7d6c5c6cd453ac#diff-a00aec43f56686c876d5fec8bb227e10)
で仕様が変わっているように見えます。

例外として、 `direct_io` をマウントオプションとして指定していた場合、または `direct_io` フラグを `open` の戻り値として指定した場合、
カーネルは `read(2)` システムコールの戻り値としてファイルシステムの戻り値を直接使うので、ファイルシステムは実際に読み込んだ長さを返します。  
諸事情(ストリーミングしてる等の理由)でファイルサイズと実際のデータの長さが異なる場合に、このオプションが利用できます。

引数の `fh` は `open` 時に戻り値としてファイルシステムが指定した値です。同じファイルに対して複数の `open` が来たときに、
どの `open` に対しての `read` かを識別したり、ファイルオープン毎に状態を持つことができます。  
今回は `open` を実装していないので常に0が来ます。

```rust
fn read(&mut self, _req: &Request, ino: u64, _fh: u64, _offset: i64, _size: u32, reply: ReplyData) {
    let mut data: Vec<u8> = Vec::with_capacity(_size as usize);
    let block_size: u32 = self.db.get_db_block_size();
    let mut size: u32 = _size;
    let mut offset: u32 = _offset as u32;
    // sizeが0になるまで1ブロックずつ読み込む
    while size > 0 {
        let mut b_data = match self.db.get_data(ino as u32, offset / block_size + 1, block_size) {
            Ok(n) => n,
            Err(_err) => {reply.error(ENOENT); return; }
        };
        // ブロックの途中から読み込む、またはブロックの途中まで読む場合の対応
        let b_offset: u32 = offset % block_size;
        let b_end: u32 = if (size + b_offset) / block_size >= 1 {block_size} else {size + b_offset};
        if b_data.len() < b_end as usize {
            b_data.resize(b_end as usize, 0);
        }
        data.append(&mut b_data[b_offset as usize..b_end as usize].to_vec());
        offset += b_end - b_offset;
        size -= b_end - b_offset;
    }
    reply.data(&data);
}
```

## readdir
```rust
fn readdir(&mut self, _req: &Request, ino: u64, _fh: u64, offset: i64, mut reply: ReplyDirectory);
```

指定されたinodeのディレクトリのディレクトリエントリを返します。  
ディレクトリ内の全てのファイルまたはディレクトリの、
「名前」、「ファイル種別(ファイル/ディレクトリ/シンボリックリンク/etc.)」、「inode番号」を返します。  
`ls` コマンドの結果を返すイメージです。  
一定サイズのバッファが渡されるので、一杯になるまでディレクトリエントリを入れて返します。

引数の `fh` は `opendir` でファイルシステムが渡した値です。今回は `opendir` を実装していないので0です。

cでは `fuse_add_direntry()` という関数を使用してバッファを埋めますが、rustでは引数で渡された `reply: ReplyDirectory` を使用します。  
具体的には以下のように使います。

```rust
let target_inode = 11; // inode番号
let filename = "test.txt"; // ファイル名
let fileType = FileType.RegularFile; //ファイル種別
result = reply.add(target_inode, offset, fileType, filename);
```

`reply.add()` でバッファにデータを追加していき、最終的に `reply.ok()` を実行すると、データが返せます。

バッファが一杯の時、 `ReplyDirectory.add()` は `true` を返します。

`reply.add()` の引数の `offset` はファイルシステムが任意に決めたオフセットです。  
大抵はディレクトリエントリ一覧内のインデックスや次のエントリへのポインタ(cの場合)が使われます。
同じディレクトリエントリ内で `offset` は一意でなければなりません。また、offsetは決まった順番を持たなければなりません。  
カーネルが`readdir`の 引数として `offset` に0でない値を指定してきた場合、
該当の `offset` を持つディレクトリエントリの、次のディレクトリエントリを返さなければならないからです。  
`readdir` の引数に `0` が来た場合「最初のディレクトリエントリ」を返さないといけないので、ファイルシステムは `offset` に0を入れてはならないです。

厳密に実装する場合、 `opendir` 時の状態を返さないといけないので、 `opendir` の実装と状態の保持が必要になります。

`.` と `..` は返さなくともよいですが、返さなかった場合の処理は呼び出し側のプログラムに依存します。

```rust
fn readdir(&mut self, _req: &Request, ino: u64, _fh: u64, offset: i64, mut reply: ReplyDirectory) {
    let db_entries: Vec<DEntry> = match self.db.get_dentry(ino as u32) {
        Ok(n) => n,
        Err(err) => {reply.error(ENOENT); debug!("{}", err); return;}
    };
    for (i, entry) in db_entries.into_iter().enumerate().skip(offset as usize) {
        let full = reply.add(entry.child_ino as u64, (i + 1) as i64, entry.file_type, &entry.filename);
        if full {
            break;
        }
    }
    reply.ok();
}
```

## マウント
main関数で `fuse::mount()` を実行すると、マウントできます。

fuse-rsは [env_logger](https://github.com/sebasmagri/env_logger/) に対応しているので、最初に有効にしておきます。  
`RUST_LOG=debug [コマンド]` のように、環境変数でレベルを設定できます。  
`DEBUG` レベルにすると各関数の呼び出しを記録してくれます。

引数は雑に取得していますが、
マウントオプションの処理などがあるので、後々 [clap](https://github.com/clap-rs/clap) などを使って解析することにします。

```rust
fn main() {
    // ログを有効にする
    env_logger::init();
    // 引数からマウントポイントを取得
    let mountpoint = env::args_os().nth(1).unwrap();
    // 引数からDBファイルのパスを取得
    let db_path = env::args_os().nth(2).unwrap();
    // マウントオプションの設定
    let options = ["-o", "ro", "-o", "fsname=sqlitefs"]
        .iter()
        .map(|o| o.as_ref())
        .collect::<Vec<&OsStr>>();
    // ファイルシステムの初期化
    let fs: SqliteFs = match SqliteFs::new(db_path.to_str().unwrap()) {
        Ok(n) => n,
        Err(err) => {println!("{:?}", err); return;}
    };
    // マウント
    fuse::mount(fs, &mountpoint, &options).unwrap();
}
```

## 初期データ登録
自動でテーブルを作成する機能をまだ実装していません。初期化用の `init.sql` と、hello.txt追加用の `hello.sql` 
がソースコードに付属しているので、実行してデータベースを作成します。

```text
$ sqlite3 ~/filesystem.sqlite < init.sql
$ sqlite3 ~/filesystem.sqlite < hello.sql
```

## ビルド及び実行
`[プログラム名] [マウント先] [データベースファイル名]` で実行できます。

```text:コマンド
# バックグラウンドで実行
$ ./sqlite-fs ~/mount ~/filesystem.sqlite &
# lsしてhello.txtがあるのを確認
$ ls ~/mount
hello.txt
# hello.txtの内容が読み込めることを確認
$ cat ~/mount/hello.txt
Hello World!
```

また、 `$ RUST_LOG=debug cargo run ~/mount` でビルドと実行( `~/mount` にマウントして、デバッグログを出力)ができます。  
試しに `cat ~/mount/hello.txt` を実行すると、以下のようなログが出力されます。 `env_logger` のおかげで各関数に対する呼び出しが記録されています。

```text:ログ
[2019-10-25T10:43:27Z DEBUG fuse::request] INIT(2)   kernel: ABI 7.31, flags 0x3fffffb, max readahead 131072
[2019-10-25T10:43:27Z DEBUG fuse::request] INIT(2) response: ABI 7.8, flags 0x1, max readahead 131072, max write 16777216
[2019-10-25T10:43:42Z DEBUG fuse::request] LOOKUP(4) parent 0x0000000000000001, name "hello.txt"
[2019-10-25T10:43:42Z DEBUG fuse::request] OPEN(6) ino 0x0000000000000002, flags 0x8000
[2019-10-25T10:43:42Z DEBUG fuse::request] READ(8) ino 0x0000000000000002, fh 0, offset 0, size 4096
[2019-10-25T10:43:42Z DEBUG fuse::request] FLUSH(10) ino 0x0000000000000002, fh 0, lock owner 12734418937618606797
[2019-10-25T10:43:42Z DEBUG fuse::request] RELEASE(12) ino 0x0000000000000002, fh 0, flags 0x8000, release flags 0x0, lock owner 0
```

lookup -> open -> read -> close の順で関数が呼び出されている事が分かります。  
`close` に対応する関数である `flush` と `release` は実装していませんが、動作しています。

ファイルシステムは `fusermount -u [マウント先]` でアンマウントできます。アンマウントするとプログラムは終了します。  
`Ctrl + c` 等でプログラムを終了した場合でもマウントしたままになっているので、かならず `fusermount` を実行してください。

## まとめ
4つの関数を実装するだけで、Readonlyのファイルシステムが作成できました。  
次回はファイルにデータの書き込みができるようにします。

# ReadWrite
## 概要
前回は、ファイルの読み込みができるファイルシステムを作成しました。
今回は、それに加えてファイルの書き込みができるようにします。

必要なのは以下の関数です。

```rust
fn write(&mut self, _req: &Request<'_>, _ino: u64, _fh: u64, _offset: i64, _data: &[u8], _flags: u32, reply: ReplyWrite) {
    ...
}

fn setattr(&mut self, _req: &Request<'_>, _ino: u64, _mode: Option<u32>, _uid: Option<u32>, _gid: Option<u32>, _size: Option<u64>, _atime: Option<Timespec>, _mtime: Option<Timespec>, _fh: Option<u64>, _crtime: Option<Timespec>, _chgtime: Option<Timespec>, _bkuptime: Option<Timespec>, _flags: Option<u32>, reply: ReplyAttr) {
    ...
}
```

なお、以下では実装する関数と同名のシステムコールと区別をつけるために、 システムコールは `write(2)` のような表記をします。

## DB関数
今回追加したDB側の関数は以下になります。

```
    /// inodeのメタデータを更新する。ファイルサイズが縮小する場合はtruncateをtrueにする
    fn update_inode(&self, attr: DBFileAttr, truncate: bool) -> Result<(), Error>;

    /// 1ブロック分のデータを書き込む
    fn write_data(&self, inode:u32, block: u32, data: &[u8], size: u32) -> Result<(), Error>;
```

## write

```rust
fn write(&mut self, _req: &Request<'_>, ino: u64, fh: u64, offset: i64, data: &[u8], flags: u32, reply: ReplyWrite);
```

引数の `inode` で指定されたファイルに、引数の `data` で渡ってきたデータを書き込みます。

`write(2)` のようなシステムコールを使う場合はファイルオフセットを意識する必要がありますが、
FUSEではカーネルがオフセットの管理をしてくれているので、 `pwrite(2)` 相当の関数を一つ実装するだけで済むようになっています。

マウントオプションに `direct_io` が設定されていない場合、エラーを返す場合を除いて、writeはsizeで指定された数字をreplyで返さないといけません。  
指定されている場合は、実際に書き込んだバイト数を返します。

引数の `fh` は `open` 時にファイルシステムが指定した値です。今回はまだopenを実装していないので、常に0になります。

また、 `open` 時のフラグに `O_APPEND` が設定されている場合は適切に処理しなければなりません。

ちなみに、cpでファイルを上書きすると頭から後ろまで`write` 関数が実行されますが、
アプリケーションによってはファイルの一部分だけ更新する、という処理はよく発生します。書き込みをキャッシュしたりしている場合は気をつけてください。

### O_APPEND
ライトバックキャッシュが有効か無効かの場合で動作が異なります。
マウントオプションに `-o writeback` がある場合、ライトバックキャッシュが有効になっています。

ライトバックキャッシュが無効の時、ファイルシステムは `O_APPEND` を検知して、
全ての `write` の中で `offset` の値にかかわらずデータがファイル末尾に追記されるようにチェックします。

ライトバックキャッシュが有効の時、 `offset` はカーネルが適切に設定してくれます。 `O_APPEND` は無視してください。

(今のところ)カーネルは `offset` をきちんと設定してくれるようです。  
なので、現状は `O_APPEND` は無視し、 `open` 実装時に対応します。  
ただし、ネットワーク上のストレージを利用しているファイルシステムなどで複数のマシンから `O_APPEND` で書き込みがあった場合、カーネルの認知しているファイル末尾と
実際のファイル末尾がずれるので、問題が発生します。  
こういった問題が発生しうるファイルシステムを作る場合は、対処する必要があります。

### ここまでのコード
```rust
fn write(&mut self, _req: &Request<'_>, ino: u64, _fh: u64, offset: i64, data: &[u8], flags: u32, reply: ReplyWrite) {
    let block_size = self.db.get_db_block_size();
    let ino = ino as u32;
    let size = data.len() as u32;
    let offset = offset as u32;
    let start_block = offset / block_size + 1;
    let end_block = (offset + size - 1) / block_size + 1;
    // 各ブロックに書き込む
    for i in start_block..=end_block {
        let mut block_data: Vec<u8> = Vec::with_capacity(block_size as usize);
        let b_start_index = if i == start_block {offset % block_size} else {0};
        let b_end_index = if i == end_block {(offset+size-1) % block_size +1} else {block_size};
        let data_offset = (i - start_block) * block_size;

        // 書き込みがブロック全体に及ばない場合、一度ブロックのデータを読み込んで隙間を埋める
        if (b_start_index != 0) || (b_end_index != block_size) {
            let mut data_pre = match self.db.get_data(ino, i, block_size) {
                Ok(n) => n,
                Err(err) => {reply.error(ENOENT); debug!("{}", err); return;}
            };
            if data_pre.len() < block_size as usize {
                data_pre.resize(block_size as usize, 0);
            }
            if b_start_index != 0 {
                block_data.extend_from_slice(&data_pre[0..b_start_index as usize]);
            }
            block_data.extend_from_slice(&data[data_offset as usize..(data_offset + b_end_index - b_start_index) as usize]);
            if b_end_index != block_size {
                block_data.extend_from_slice(&data_pre[b_end_index as usize..block_size as usize]);
            }
        } else {
            block_data.extend_from_slice(&data[data_offset as usize..(data_offset + block_size) as usize]);
        }
        // ここで書き込む
        match self.db.write_data(ino, i, &block_data, (i-1) * block_size + b_end_index) {
            Ok(n) => n,
            Err(err) => {reply.error(ENOENT); debug!("{}", err); return;}
        }
    }
    reply.written(size);
}
```

### マウントオプション
`main.rs` のReadOnlyのマウントオプションを削除します。

```
let options = ["-o", "fsname=sqlitefs"]
    .iter()
    .map(|o| o.as_ref())
    .collect::<Vec<&OsStr>>();
```

### 実行結果
- コマンド

```
// hello.txtに追記する
$ echo "append" >> ~/mount/hello.txt
// 追記された内容の確認
$ cat ~/mount/hello.txt
Hello world!
append
```

- FUSEログ

```
[2019-10-28T11:52:08Z DEBUG fuse::request] INIT(2)   kernel: ABI 7.31, flags 0x3fffffb, max readahead 131072
[2019-10-28T11:52:08Z DEBUG fuse::request] INIT(2) response: ABI 7.8, flags 0x1, max readahead 131072, max write 16777216
[2019-10-28T11:52:14Z DEBUG fuse::request] LOOKUP(4) parent 0x0000000000000001, name "hello.txt"
[2019-10-28T11:52:14Z DEBUG fuse::request] OPEN(6) ino 0x0000000000000002, flags 0x8401
[2019-10-28T11:52:14Z DEBUG fuse::request] FLUSH(8) ino 0x0000000000000002, fh 0, lock owner 9742156966771960265
[2019-10-28T11:52:14Z DEBUG fuse::request] GETXATTR(10) ino 0x0000000000000002, name "security.capability", size 0
[2019-10-28T11:52:14Z DEBUG fuse::request] WRITE(12) ino 0x0000000000000002, fh 0, offset 13, size 7, flags 0x0
[2019-10-28T11:52:14Z DEBUG fuse::request] RELEASE(14) ino 0x0000000000000002, fh 0, flags 0x8401, release flags 0x0, lock owner 0
```


## read, write時のメタデータの更新
一般的にファイルを `read` した時にはメタデータの `atime` を更新します。  
また、 `write` した時には、 `size` (必要な場合) `mtime` `ctime` の3つを更新します。  
DB関数側でこれらを更新できるようにしておきます。

今回はメタデータもデータもデータベース上にあり、元々パフォーマンスが悪い事が予想されるのであまり意識していませんが、
メタデータの更新にある程度コストがかかる場合、ファイルサイズとタイムスタンプはメモリ上にキャッシュした方がいいです。

マウントオプションで `-o noatime` が指定された場合、 `atime` の更新は行いません。

### ファイルサイズ
`write` 時に書き込まれたデータの末尾が、既存のファイルサイズより大きくなる場合は、ファイルサイズを更新する必要があります。  
また、書き込みのオフセットにファイルサイズより大きい値が指定された場合、ファイルに何も書かれていない穴ができます。
このエリアのデータが読まれた場合、ファイルシステムは0(NULLバイト)の列を返します。

ファイルのブロックをばらばらの順番で書き込む、というのはよくある処理なので、0バイト目から順番に書き込まれていく事は期待しないでください。  
特に `write` 実行時にファイルサイズを変更する場合は、現在のファイルサイズより小さくならないように気をつけてください。  

### タイムスタンプ
各関数とどのタイムスタンプを更新すべきかの対応表です。  
[Linuxプログラミングインターフェース](https://www.oreilly.co.jp/books/9784873115856/)
のシステムコールとタイムスタンプの対応表を参考に、FUSEの関数にマップしました。  
左側の `a, m, c` が操作対象のファイルまたはディレクトリのタイムスタンプ、
右側の `a, m, c` は親ディレクトリのタイムスタンプです。

|関数名|a|m|c|親a|親m|親c|備考|
|---|---|---|---|---|---|---|---|
|setattr| | |o| | | | |
|setattr(*)| |o|o| | | | * ファイルサイズが変わる場合 |
|link| | |o| |o|o| |
|mkdir|o|o|o| |o|o| |
|mknod|o|o|o| |o|o| |
|create|o|o|o| |o|o| |
|read|o| | | | | | |
|readdir|o| | | | | | |
|setxattr| | |o| | | | |
|removexattr| | |o| | | | |
|rename| | |o| |o|o|移動前/移動後の両方の親ディレクトリを変更|
|rmdir| | | | |o|o| |
|symlink|o|o|o| |o|o|リンク自体のタイムスタンプで、リンク先は変更しない|
|unlink| | |o| |o|o|参照カウントが2以上でinode自体が消えない場合、ファイルのctimeを更新|
|write| |o|o| | | | |

## setattr

```rust
fn setattr(&mut self, _req: &Request<'_>, ino: u64, mode: Option<u32>, uid: Option<u32>, gid: Option<u32>, size: Option<u64>, atime: Option<Timespec>, mtime: Option<Timespec>, fh: Option<u64>, crtime: Option<Timespec>, chgtime: Option<Timespec>, bkuptime: Option<Timespec>, flags: Option<u32>, reply: ReplyAttr);
```

`write` は実装しましたが、このままでは追記しかできません。  
ファイルを丸ごと更新するために、ファイルサイズを0にする(truncateに相当) 処理を実装します。

fuse-rsでは、 `setattr` を実装する事でファイルサイズの変更が可能になります。  
ついでに `setattr` で変更できる全てのメタデータを変更できるようにします。

`truncate(2)` でファイルサイズを変更する時と、 `open(2)` で `O_TRUNC` を指定した時も、この関数が呼ばれます。

`setattr` は各引数に `Option` 型で値が指定されるので、中身がある場合はその値で更新していきます。  
`reply` に入れる値は、更新後のメタデータです。

なお、 `ctime` は 現在のfuse-rsがプロトコルのバージョンの問題で未対応なので、引数には入っていません。  
基本的に `ctime` は自由に設定する事ができず、 `setattr` を実行すると現在時刻になるはずなので、問題はありません。

`open` 時に `O_TRUNC` を指定した場合のように、ファイルサイズに0が指定された場合は既存のデータを全て破棄すればいいですが、
`truncate(2)` で元のファイルサイズより小さい0以外の値が指定された場合、
残すべきデータは残しつついらないデータがきちんと破棄されるように気をつけてください。  
また、元のファイルサイズより大きい値が指定された場合、間のデータが0(\0)で埋められるようにしてください。

macOS用に `chgtime` と `bkuptime` が引数にありますが、今回はスルーします。

```rust
fn setattr(
    &mut self,
    _req: &Request<'_>,
    ino: u64,
    mode: Option<u32>,
    uid: Option<u32>,
    gid: Option<u32>,
    size: Option<u64>,
    atime: Option<Timespec>,
    mtime: Option<Timespec>,
    _fh: Option<u64>,
    crtime: Option<Timespec>,
    _chgtime: Option<Timespec>,
    _bkuptime: Option<Timespec>,
    flags: Option<u32>,
    reply: ReplyAttr
) {
    // 現在のメタデータを取得
    let mut attr = match self.db.get_inode(ino as u32) {
        Ok(n) => n,
        Err(err) => {reply.error(ENOENT); debug!("{}", err); return;}
    };
    // ファイルサイズの変更チェック
    let old_size = attr.size;
    // 引数で上書き
    if let Some(n) = mode {attr.perm = n as u16};
    if let Some(n) = uid {attr.uid = n};
    if let Some(n) = gid {attr.gid = n}; 
    if let Some(n) = size {attr.size = n as u32};
    if let Some(n) = atime {attr.atime = datetime_from_timespec(&n)};
    if let Some(n) = mtime {attr.mtime = datetime_from_timespec(&n)};
    if let Some(n) = crtime {attr.crtime = datetime_from_timespec(&n)};
    if let Some(n) = flags {attr.flags = n};
    // 更新
    match self.db.update_inode(attr, old_size > attr.size) {
        Ok(_n) => (),
        Err(err) => {reply.error(ENOENT); debug!("{}", err); return;}
    };
    reply.attr(&ONE_SEC, &attr.get_file_attr());
}
```

実行結果は以下のようになります。

```
$ echo "Update hello world" > ~/mount/hello.txt
$ cat ~/mount/hello.txt
Update hello world
```

```
[2019-10-28T12:08:10Z DEBUG fuse::request] INIT(2)   kernel: ABI 7.31, flags 0x3fffffb, max readahead 131072
[2019-10-28T12:08:10Z DEBUG fuse::request] INIT(2) response: ABI 7.8, flags 0x1, max readahead 131072, max write 16777216
[2019-10-28T12:08:37Z DEBUG fuse::request] LOOKUP(4) parent 0x0000000000000001, name "hello.txt"
[2019-10-28T12:08:37Z DEBUG fuse::request] OPEN(6) ino 0x0000000000000002, flags 0x8001
[2019-10-28T12:08:37Z DEBUG fuse::request] GETXATTR(8) ino 0x0000000000000002, name "security.capability", size 0
[2019-10-28T12:08:37Z DEBUG fuse::request] SETATTR(10) ino 0x0000000000000002, valid 0x208
[2019-10-28T12:08:37Z DEBUG fuse::request] FLUSH(12) ino 0x0000000000000002, fh 0, lock owner 17171727478964840688
[2019-10-28T12:08:37Z DEBUG fuse::request] WRITE(14) ino 0x0000000000000002, fh 0, offset 0, size 19, flags 0x0
[2019-10-28T12:08:37Z DEBUG fuse::request] RELEASE(16) ino 0x0000000000000002, fh 0, flags 0x8001, release flags 0x0, lock owner 0

```

## まとめ
これでファイルの書き込み(追記、上書き)ができるようになりました。  
次回は、ファイルの作成と削除を実装します。

# ファイルの作成と削除
## 概要
ルートディレクトリ上にファイルの作成と削除が行えるようにします。

必要なのは以下の関数です。

```
fn init(&mut self, _req: &Request<'_>) -> Result<(), c_int> {
    ...
}
fn destroy(&mut self, _req: &Request<'_>) {
    ...
}
fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry){
    ...(既存のコードに追加)
}
fn forget(&mut self, _req: &Request<'_>, _ino: u64, _nlookup: u64) {
    ...
}
fn unlink(&mut self, _req: &Request<'_>, _parent: u64, _name: &OsStr, reply: ReplyEmpty) {
    ...
}
fn create(&mut self, _req: &Request<'_>, _parent: u64, _name: &OsStr, _mode: u32, _flags: u32, reply: ReplyCreate) {
    ...
}
```

ファイルを作成する関数は `create` 、削除する関数は `unlink` ですが、 `lookup count` の都合で追加でいろいろ実装する必要があります。

## lookup count
ファイル削除に絡む関数を実装する場合、 `lookup count` に注意する必要があります。

以下では lookup count と参照カウントという2種類の言葉を使っていますが、  
大まかに説明すると、lookup count はいくつのプロセスがファイルを開いているか(または開く予定か)、  
参照カウントはファイルがいくつハードリンクされているか、を示しています。

`lib.rs` や `fuse_lowlevel.h` によると、
「lookup count が0でない内は、unlink, rmdir, rename(で上書き)されて参照カウントが0になってもinodeを削除しないでね」という事です。  
lookup countは最初は0で、ReplyEntryとReplyCreateがある全ての関数が呼ばれるたびに、1ずつ増やしていきます。  
具体的には、 `lookup`, `mknod`, `mkdir`, `symlink`, `link`, `create` が実行されると1増えます。

`forget` はlookup countを減らす関数です。 `forget` でlookup countが0になるまでは、ファイルシステムは削除を遅らせる必要があります。  
例えば、カーネルはまだファイルをOpenしているプロセスがあると、 `forget` をファイルが閉じられるまで遅延させます。  
これにより、「別の誰かがファイルを削除して `ls` 等でファイルを見つけられなくなるが、削除前からファイルを開いていた場合は読み込み続ける事ができる」というアレが実現できます。

lookup countを実装するために、ファイルシステムの構造体に次の変数を追加します。

```rust
pub struct SqliteFs{
    /// DBとやり取りする
    db: Sqlite,
    /// lookup countを保持する
    lookup_count: Mutex<HashMap<u32, u32>>
}
```

keyがinode番号、valueがlookup count、であるHashMapを作成します。

## 追加したDB関数
今回は以下のようなDB関数を追加しました。

```rust
/// ファイル/ディレクトリのinodeを追加し、新しく割り振ったinode番号を返す。 引数attrのinoは無視される。
fn add_inode(&mut self, parent: u32, name: &str, attr: &DBFileAttr) -> Result<u32, Error>;
/// inodeをチェックし、参照カウントが0なら削除する
fn delete_inode_if_noref(&mut self, inode: u32) -> Result<(), Error>;
/// 親ディレクトリのinode番号、ファイル/ディレクトリ名で指定されたディレクトリエントリを削除し、
/// 該当のinodeの参照カウントを1減らす
/// 削除したファイル/ディレクトリのinode番号を返す
fn delete_dentry(&mut self, parent: u32, name: &str) -> Result<u32, Error>;
/// 参照カウントが0である全てのinodeを削除する
fn delete_all_noref_inode(&mut self) -> Result<(), Error>;
```

## lookup
`lookup` 関数を更新します。  
関数が実行されるたびに、lookupで情報を持ってくる対象のファイル/ディレクトリのlookup countに1を足すようにします。

コードは以下のようになります。

```rust
fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
    // 既存のコード
    let parent = parent as u32;
    let child = match self.db.lookup(parent, name.to_str().unwrap()) {
        Ok(n) => {
            if n.ino == 0 {
                reply.error(ENOENT);
                return;
            }
            reply.entry(&ONE_SEC, &n.get_file_attr() , 0);
            n.ino
        },
        Err(err) => {reply.error(ENOENT); debug!("{}", err); return;}
    };

    // *update* lookup countに1を足す。HashMapにkeyが無い場合は追加する
    let mut lc_list = self.lookup_count.lock().unwrap();
    let lc = lc_list.entry(child).or_insert(0);
    *lc += 1;
}
```

## create

```rust
fn create(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, mode: u32, flags: u32, reply: ReplyCreate);
```

引数の `parent` のinode番号で指定されたディレクトリ内の、 `name` で指定されたファイル名を持つファイルを作成します。

`creat(2)` または `O_CREAT` を指定した `open(2)` 実行時に呼ばれます。

指定されたファイルが存在しない場合、引数の `mode` で指定されたモードでファイルを作成し、ファイルを開きます。  
ファイルのオーナーに設定するユーザ、グループは、引数の `req` から `req.uid()` `req.gid()` で取得できます。  
ただし、マウントオプションで `–o grpid` または `–o bsdgroups` が指定されている場合や、親ディレクトリにsgidが設定されている場合( `libc::S_ISGID` で判定)は、
親ディレクトリと同じグループを設定しないといけません。

ファイルが既に存在する場合、openと同じ動作を行います。

ファイルを作成する以外は `open` と同じ動作のため、open時のフラグが `flags` で渡されます。

`creat(2)` や `open(2)` ではフラグに `O_EXCL` が指定されている場合、指定した名前のファイルが既に存在するとエラーにしなければなりませんが、
この処理はカーネルがやってくれているようです。

`create` が実装されていない場合、カーネルは `mknod` と `open` を実行します。

なお、`create` が実装されている場合、libfuseは通常ファイルの `mknod` が実行されると `create` を呼び出しますが、
fuse-rsは呼び出してくれません。

実装したコードは以下のようになります。

```rust
fn create(
    &mut self,
    req: &Request<'_>,
    parent: u64,
    name: &OsStr,
    mode: u32,
    _flags: u32,
    reply: ReplyCreate
) {
    let ino;
    let parent = parent as u32;
    let name = name.to_str().unwrap();
    // ファイルが既にあるかチェックする
    let mut attr = match self.db.lookup(parent, name) {
        Ok(n) => n,
        Err(err) => {reply.error(ENOENT); debug!("{}", err); return;}
    };
    if attr.ino == 0 {
        // ファイル作成
        let now = SystemTime::now();
        attr = DBFileAttr {
            ino: 0, // 無視されるので0にする
            size: 0,
            blocks: 0,
            atime: now,
            mtime: now,
            ctime: now,
            crtime: now,
            kind: FileType::RegularFile,
            perm: mode as u16,
            nlink: 0,
            uid: req.uid(),
            gid: req.gid(),
            rdev: 0,
            flags: 0
        };
        ino = match self.db.add_inode(parent, name, &attr) {
            Ok(n) => n,
            Err(err) => {
                reply.error(ENOENT);
                debug!("{}", err);
                return;
            }
        };
        attr.ino = ino;
    } else {
        ino = attr.ino;
    }
    // createもlookup countを+1する
    let mut lc_list = self.lookup_count.lock().unwrap();
    let lc = lc_list.entry(ino).or_insert(0);
    *lc += 1;
    reply.created(&ONE_SEC, &attr.get_file_attr(), 0, 0, 0);
}
```

## unlink

```rust
fn unlink(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEmpty);
```

親ディレクトリのinode番号が引数の `parent`, 削除対象のファイル/ディレクトリの名前が `name` で指定されるので、
ファイルまたはディレクトリを削除します。

削除対象はディレクトリエントリと、該当のinodeのメタデータです。
ただし、inodeはハードリンクされて複数のディレクトリエントリから参照されている可能性があるので、参照カウント( `nlink` ) を1減らし、0になった場合に削除します。  
また、 lookup count をチェックし、0になっていない場合は即座に削除を行いません。

```rust
fn unlink(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEmpty) {
    // ディレクトリエントリを削除しつつ、対象のinode番号を得る
    let ino = match self.db.delete_dentry(parent as u32, name.to_str().unwrap()) {
        Ok(n) => n,
        Err(err) => {reply.error(ENOENT); debug!("{}", err); return;}
    };
    // lookup countのチェック
    let lc_list = self.lookup_count.lock().unwrap();
    if !lc_list.contains_key(&ino) {
        // 参照カウントが0の場合削除する。そうでない場合、unlink 内では削除しない
        match self.db.delete_inode_if_noref(ino) {
            Ok(n) => n,
            Err(err) => {reply.error(ENOENT); debug!("{}", err); return;}
        };
    }
    reply.ok();
}
```

## forget

```rust
fn forget(&mut self, _req: &Request<'_>, _ino: u64, _nlookup: u64);
```

lookup countを減らします。

引数の `ino` で対象のinode番号、 `nlookup` で減らす数が指定されます。  
inodeの削除が遅延されている場合、lookup countが0になったタイミングで削除します。

```rust
fn forget(&mut self, _req: &Request<'_>, ino: u64, nlookup: u64) {
    let ino = ino as u32;
    // lookup countのチェック
    let mut lc_list = self.lookup_count.lock().unwrap();
    let lc = lc_list.entry(ino).or_insert(0);
    *lc -= nlookup as u32;
    if *lc == 0 {
        // 0(以下)になった場合、lookup countの一覧から削除する
        lc_list.remove(&ino);
        // 参照カウントが0でinodeの削除が遅延されていた場合、改めて削除する
        match self.db.delete_inode_if_noref(ino) {
            Ok(n) => n,
            Err(err) => debug!("{}", err)
        }
    }
}
```

## destroy

```rust
fn destroy(&mut self, _req: &Request<'_>);
```

ファイルシステムの終了時に呼ばれる関数です。

ファイルシステムのアンマウント時には、全ての lookup count が0になる事が期待されます。  
一方、 `forget` が呼ばれる事は保証されていないので、ファイルシステムが自分でチェックする必要があります。

```rust
fn destroy(&mut self, _req: &Request<'_>) {
    let lc_list = self.lookup_count.lock().unwrap();
    // lookup countが残っている全てのinodeをチェック
    for key in lc_list.keys() {
        // 参照カウントが0でinodeの削除が遅延されていた場合、改めて削除する
        match self.db.delete_inode_if_noref(*key) {
            Ok(n) => n,
            Err(err) => debug!("{}", err)
        }
    }
}
```

## init

```rust
fn init(&mut self, _req: &Request<'_>) -> Result<(), c_int>;
```

ファイルシステムのマウント時に最初に呼ばれる関数です。

何らかの事情で `destroy` が呼ばれずにファイルシステムが突然終了した場合、参照カウントが0のままのinodeが残り続ける事になるので、
チェックして削除します。

```rust
fn init(&mut self, _req: &Request<'_>) -> Result<(), c_int> {
    match self.db.delete_all_noref_inode() {
        Ok(n) => n,
        Err(err) => debug!("{}", err)
    };
    Ok(())
}
```

## 実行結果
### ファイル作成

- コマンド

```
# ファイル作成
$ touch ~/mount/touch.txt
# ファイル作成 + 書き込み
$ echo "created" > ~/mount/test.txt
# ファイル作成の確認
$ ls ~/mount
hello.txt  test.txt  touch.txt
# 書き込み内容の確認
$ cat ~/mount/test.txt
created
```

- FUSEログ

```
[2019-10-30T11:21:59Z DEBUG fuse::request] INIT(2)   kernel: ABI 7.31, flags 0x3fffffb, max readahead 131072
[2019-10-30T11:21:59Z DEBUG fuse::request] INIT(2) response: ABI 7.8, flags 0x1, max readahead 131072, max write 16777216
// touch ~/mount/touch.txt
[2019-10-30T11:22:14Z DEBUG fuse::request] LOOKUP(4) parent 0x0000000000000001, name "touch.txt"
[2019-10-30T11:22:14Z DEBUG fuse::request] CREATE(6) parent 0x0000000000000001, name "touch.txt", mode 0o100664, flags 0x8841
[2019-10-30T11:22:14Z DEBUG fuse::request] FLUSH(8) ino 0x0000000000000003, fh 0, lock owner 16194556409419452441
[2019-10-30T11:22:14Z DEBUG fuse::request] SETATTR(10) ino 0x0000000000000003, valid 0x1b0
[2019-10-30T11:22:14Z DEBUG fuse::request] RELEASE(12) ino 0x0000000000000003, fh 0, flags 0x8801, release flags 0x0, lock owner 0
// echo "created" > ~/mount/test.txt
[2019-10-30T11:22:28Z DEBUG fuse::request] LOOKUP(14) parent 0x0000000000000001, name "test.txt"
[2019-10-30T11:22:28Z DEBUG fuse::request] CREATE(16) parent 0x0000000000000001, name "test.txt", mode 0o100664, flags 0x8241
[2019-10-30T11:22:29Z DEBUG fuse::request] GETXATTR(18) ino 0x0000000000000004, name "security.capability", size 0
[2019-10-30T11:22:29Z DEBUG fuse::request] WRITE(20) ino 0x0000000000000004, fh 0, offset 0, size 8, flags 0x0
[2019-10-30T11:22:29Z DEBUG fuse::request] RELEASE(22) ino 0x0000000000000004, fh 0, flags 0x8001, release flags 0x0, lock owner 0
```

### ファイル削除

- コマンド

```
$ rm ~/mount/test.txt
$ ls ~/mount/test.txt
ls: cannot access '/home/jiro/mount/test.txt': No such file or directory
```

- FUSEログ

```
[2019-10-30T05:32:26Z DEBUG fuse::request] LOOKUP(48) parent 0x0000000000000001, name "test.txt"
[2019-10-30T05:32:26Z DEBUG fuse::request] ACCESS(50) ino 0x0000000000000004, mask 0o002
[2019-10-30T05:32:26Z DEBUG fuse::request] UNLINK(52) parent 0x0000000000000001, name "test.txt"
[2019-10-30T05:32:26Z DEBUG fuse::request] FORGET(54) ino 0x0000000000000004, nlookup 4
```

## まとめ
ファイルの作成、削除が問題なくできるようになりました。  
次回は、ディレクトリの作成/削除ができるようにします。

# ディレクトリの作成/削除
## 概要
今まではルートディレクトリのみでファイル操作を行っていましたが、
今回はディレクトリの作成/削除を実装して、サブディレクトリでいろいろできるようにします。

## 実装すべき関数
```rust
fn mkdir(&mut self, req: &Request<'_>, parent: u64, name: &OsStr, mode: u32, reply: ReplyEntry) {
    ...
}
fn rmdir(&mut self, _req: &Request<'_>, _parent: u64, _name: &OsStr, reply: ReplyEmpty) {
    ...
}
```

## 追加したDB関数

```rust
/// ディレクトリが空かチェックする
fn check_directory_is_empty(&self, inode: u32) -> Result<bool, Error>;
```

## mkdir
```rust
fn mkdir(&mut self, req: &Request<'_>, parent: u64, name: &OsStr, mode: u32, reply: ReplyEntry);
```

引数の `parent` で親ディレクトリのinode番号、 `name` で作成するディレクトリ名、 `mode` でモードが指定されるので、ディレクトリを作成します。
成功した場合、作成したディレクトリのメタデータを返します。

作成したユーザ、グループは、引数の `req` から `req.uid()` `req.gid()` で取得できます。  
ただし、マウントオプションで `–o grpid` または `–o bsdgroups` が指定されている場合や、親ディレクトリにsgidが設定されている場合は、
親ディレクトリと同じグループを設定しないといけません。

また、親ディレクトリにSUIDが設定されていても、SUIDはディレクトリには関係ないので子ディレクトリは無視します。  
SGID, スティッキービットが設定されている場合、子ディレクトリにも引き継がないといけません。  
この辺りは処理系定義なので、Linux以外のシステムで、常にディレクトリ作成時にはoffにするものもあるようです。  

既に同名のディレクトリやファイルがあるかどうか、はカーネル側でチェックしてくれているようです。

`mkdir` のコードは以下のようになります。

```rust
fn mkdir(&mut self, req: &Request<'_>, parent: u64, name: &OsStr, mode: u32, reply: ReplyEntry) {
    let now = SystemTime::now();
    // 初期メタデータ
    let mut attr = DBFileAttr {
        ino: 0,
        size: 0,
        blocks: 0,
        atime: now,
        mtime: now,
        ctime: now,
        crtime: now,
        kind: FileType::Directory,
        perm: mode as u16,
        nlink: 0,
        uid: req.uid(),
        gid: req.gid(),
        rdev: 0,
        flags: 0
    };
    // ディレクトリを作成してinode番号を取得
    let ino =  match self.db.add_inode(parent as u32, name.to_str().unwrap(), &attr) {
        Ok(n) => n,
        Err(err) => {reply.error(ENOENT); debug!("{}", err); return;}
    };
    // inode番号を入れてメタデータを返却
    attr.ino = ino;
    reply.entry(&ONE_SEC, &attr.get_file_attr(), 0);
    // lookup countを増やす
    let mut lc_list = self.lookup_count.lock().unwrap();
    let lc = lc_list.entry(ino).or_insert(0);
    *lc += 1;
}
```

## rmdir
```rust
fn rmdir(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEmpty);
```

引数の `parent` で親ディレクトリのinode番号が、 `name` でディレクトリ名が指定されるので、ディレクトリを削除します。

ディレクトリ内になにかある場合は削除できません。  
カーネル側で確認はしてくれないようなので、ファイルシステム側でチェックを行い、ディレクトリが空ではない( `.` と `..` 以外のエントリがある) 場合はエラーを返します。  
`rmdir(2)` のmanページによると、 `ENOTEMPTY` または `EEXIST` を返します。Linuxファイルシステムでは `ENOTEMPTY` がメジャーのようです。

`unlink` と同様に、 `lookup count` が0でない場合、0になるタイミングまでinodeの削除を遅延します。  
今回のプログラムでは、 `forget` 等の内部でファイルとディレクトリの区別をしていないので、現状の実装でOKです。

```rust
fn rmdir(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEmpty) {
    let parent = parent as u32;
    let name = name.to_str().unwrap();
    let attr = match self.db.lookup(parent, name) {
        Ok(n) => n,
        Err(err) => {reply.error(ENOENT); debug!("{}", err); return;}
    };
    // ディレクトリが空かどうかチェック
    let empty = match self.db.check_directory_is_empty(attr.ino){
        Ok(n) => n,
        Err(err) => {reply.error(ENOENT); debug!("{}", err); return;}
    };
    if !empty {
        reply.error(ENOTEMPTY);
        return;
    }
    // dentry削除
    let ino = match self.db.delete_dentry(parent, name) {
        Ok(n) => n,
        Err(err) => {reply.error(ENOENT); debug!("{}", err); return;}
    };
    // lookup countの処理
    let lc_list = self.lookup_count.lock().unwrap();
    if !lc_list.contains_key(&ino) {
        match self.db.delete_inode_if_noref(ino) {
            Ok(n) => n,
            Err(err) => {
                reply.error(ENOENT);
                debug!("{}", err);
                return;
            }
        };
    }
    reply.ok();
}
```

## 実行結果
ここまでの実行結果は以下のようになります。

```shell script
# ディレクトリ作成
$ mkdir ~/mount/testdir/
$ echo "test" > ~/mount/testdir/test.txt
# 中身のあるディレクトリは削除できないことの確認
$ rmdir ~/mount/testdir
rmdir: failed to remove '/home/jiro/mount/testdir': Directory not empty
$ rm ~/mount/testdir/test.txt
# ディレクトリ削除
$ rmdir ~/mount/testdir 
```

```
// mkdir ~/mount/testdir/
[2019-10-31T06:15:19Z DEBUG fuse::request] GETATTR(146) ino 0x0000000000000001
[2019-10-31T06:15:32Z DEBUG fuse::request] LOOKUP(148) parent 0x0000000000000001, name "testdir"
[2019-10-31T06:15:32Z DEBUG fuse::request] MKDIR(150) parent 0x0000000000000001, name "testdir", mode 0o775
[2019-10-31T06:15:32Z DEBUG fuse::request] GETATTR(152) ino 0x0000000000000001
// echo "test" > ~/mount/testdir/test.txt
[2019-10-31T06:15:56Z DEBUG fuse::request] LOOKUP(154) parent 0x0000000000000001, name "testdir"
[2019-10-31T06:15:56Z DEBUG fuse::request] LOOKUP(156) parent 0x0000000000000004, name "test.txt"
[2019-10-31T06:15:56Z DEBUG fuse::request] CREATE(158) parent 0x0000000000000004, name "test.txt", mode 0o100664, flags 0x8241
[2019-10-31T06:15:56Z DEBUG fuse::request] WRITE(160) ino 0x0000000000000005, fh 0, offset 0, size 5, flags 0x0
[2019-10-31T06:15:56Z DEBUG fuse::request] RELEASE(162) ino 0x0000000000000005, fh 0, flags 0x8001, release flags 0x0, lock owner 0
[2019-10-31T06:15:56Z DEBUG fuse::request] GETATTR(164) ino 0x0000000000000001
// rmdir(1回目)
[2019-10-31T06:16:07Z DEBUG fuse::request] LOOKUP(166) parent 0x0000000000000001, name "testdir"
[2019-10-31T06:16:07Z DEBUG fuse::request] RMDIR(168) parent 0x0000000000000001, name "testdir"
[2019-10-31T06:16:07Z DEBUG fuse::request] GETATTR(170) ino 0x0000000000000001
// rm test.txt
[2019-10-31T06:16:24Z DEBUG fuse::request] LOOKUP(174) parent 0x0000000000000001, name "testdir"
[2019-10-31T06:16:24Z DEBUG fuse::request] LOOKUP(176) parent 0x0000000000000004, name "test.txt"
[2019-10-31T06:16:24Z DEBUG fuse::request] ACCESS(178) ino 0x0000000000000005, mask 0o002
[2019-10-31T06:16:24Z DEBUG fuse::request] UNLINK(180) parent 0x0000000000000004, name "test.txt"
[2019-10-31T06:16:24Z DEBUG fuse::request] FORGET(182) ino 0x0000000000000005, nlookup 2
[2019-10-31T06:16:24Z DEBUG fuse::request] GETATTR(184) ino 0x0000000000000001
// rmdir(2回目)
[2019-10-31T06:16:34Z DEBUG fuse::request] LOOKUP(186) parent 0x0000000000000001, name "testdir"
[2019-10-31T06:16:34Z DEBUG fuse::request] RMDIR(188) parent 0x0000000000000001, name "testdir"
[2019-10-31T06:16:34Z DEBUG fuse::request] FORGET(190) ino 0x0000000000000004, nlookup 5
[2019-10-31T06:16:34Z DEBUG fuse::request] GETATTR(192) ino 0x0000000000000001
```

## まとめ
これでディレクトリの作成と削除ができるようになりました。
次回は `rename` で名前の変更と移動ができるようにします。

# 名前の変更と移動
## 概要
作成/削除の関数は一通り実装したので、今回はファイル/ディレクトリの移動ができるようにします。

## DB関数
作成したDB関数は以下になります。

```rust
/// dentryを移動させる。移動先を上書きする場合、元からあったファイル/ディレクトリのinode番号を返す
fn move_dentry(&mut self, parent: u32, name: &str, new_parent: u32, new_name: &str) -> Result<Option<u32>, Error>;
```

## rename
```rust
fn rename(&mut self, _req: &Request, parent: u64, name: &OsStr, newparent: u64, newname: &OsStr, reply: ReplyEmpty);
```

引数 `parent` で親ディレクトリのinode番号、 `name` でファイルまたはディレクトリ名、
`newparent` で変更後の親ディレクトリのinode番号,、 `newname` で変更後の名前が指定されるので、
ファイルまたはディレクトリを移動し、名前を変更します。

cの `fuse_lowlevel` の説明によると、変更先にファイルまたはディレクトリが存在する場合は自動で上書きしなければなりません。  
つまり、変更先inodeのnlinkを1減らし、ディレクトリエントリから削除します。
こちらも削除処理と同様に、 `lookup count` が0でない場合は、0になるまでinodeの削除を遅延します。  
上書き時に変更前と変更先のファイルタイプが異なる場合のチェックはカーネル側がやってくれるようで、 `rename(2)` を実行しても `rename` が呼ばれずにエラーになります。

ただし、変更前がディレクトリで、変更先のディレクトリを上書きする場合、変更先のディレクトリは空でないかファイルシステムがチェックする必要があります。  
中身がある場合は、エラーとして `ENOTEMPTY` を返します。

`rename(2)` には他に、以下の制約があります。

- 移動前と移動後のファイルが同じ(同じinode番号を指す)場合何もしない
- ディレクトリを自分自身のサブディレクトリに移動できない

この辺りはカーネルが処理してくれているようで、 `rename` 関数が呼ばれずにエラーになります。

libfuseでは上書き禁止を指定したりできる `flag` が引数に指定されますが、fuse-rsには該当する引数がありません。

```rust
fn rename(
    &mut self,
    _req: &Request<'_>,
    parent: u64,
    name: &OsStr,
    newparent: u64,
    newname: &OsStr,
    reply: ReplyEmpty
) {
    let parent = parent as u32;
    let name = name.to_str().unwrap();
    let newparent = newparent as u32;
    let newname = newname.to_str().unwrap();
    // rename. 上書きされる場合は、上書き先のinodeを取得
    let entry =  match self.db.move_dentry(parent, name, newparent, newname) {
        Ok(n) => n,
        Err(err) => match err.kind() {
            // 空の場合
            ErrorKind::FsNotEmpty {description} => {reply.error(ENOTEMPTY); debug!("{}", &description); return;},
            // ファイル -> ディレクトリの場合(カーネルがチェックしているので発生しないはず)
            ErrorKind::FsIsDir{description} => {reply.error(EISDIR); debug!("{}", &description); return;},
            // ディレクトリ -> ファイルの場合(カーネルがチェックしているので発生しないはず)
            ErrorKind::FsIsNotDir{description} => {reply.error(ENOTDIR); debug!("{}", &description); return;},
            _ => {reply.error(ENOENT); debug!("{}", err); return;},
        }
    };
    // 上書きがあった場合、各カウントを調べて、削除する必要がある場合は削除する
    if let Some(ino) = entry {
        let lc_list = self.lookup_count.lock().unwrap();
        if !lc_list.contains_key(&ino) {
            match self.db.delete_inode_if_noref(ino) {
                Ok(n) => n,
                Err(err) => {reply.error(ENOENT); debug!("{}", err); return;},
            };
        }
    }
    reply.ok();
}
```

## 実行結果

```
$ mv ~/mount/touch.txt ~/mount/touch3.txt
$ ls ~/mount/
hello.txt  testdir  touch3.txt
```

```text
[2019-10-31T10:54:17Z DEBUG fuse::request] LOOKUP(442) parent 0x0000000000000001, name "touch.txt"
[2019-10-31T10:54:17Z DEBUG fuse::request] LOOKUP(444) parent 0x0000000000000001, name "touch3.txt"
[2019-10-31T10:54:17Z DEBUG fuse::request] LOOKUP(446) parent 0x0000000000000001, name "touch3.txt"
[2019-10-31T10:54:17Z DEBUG fuse::request] LOOKUP(448) parent 0x0000000000000001, name "touch3.txt"
[2019-10-31T10:54:17Z DEBUG fuse::request] RENAME(450) parent 0x0000000000000001, name "touch.txt", newparent 0x0000000000000001, newname "touch3.txt"
[2019-10-31T10:54:17Z DEBUG fuse::request] GETATTR(452) ino 0x0000000000000001
```

```text
$ mv ~/mount/touch3.txt ~/mount/testdir
$ ls ~/mount/testdir
touch3.txt
```

```text
[2019-10-31T10:57:28Z DEBUG fuse::request] LOOKUP(500) parent 0x0000000000000001, name "touch3.txt"
[2019-10-31T10:57:28Z DEBUG fuse::request] LOOKUP(502) parent 0x0000000000000005, name "touch3.txt"
[2019-10-31T10:57:28Z DEBUG fuse::request] LOOKUP(504) parent 0x0000000000000005, name "touch3.txt"
[2019-10-31T10:57:28Z DEBUG fuse::request] LOOKUP(506) parent 0x0000000000000005, name "touch3.txt"
[2019-10-31T10:57:28Z DEBUG fuse::request] RENAME(508) parent 0x0000000000000001, name "touch3.txt", newparent 0x0000000000000005, newname "touch3.txt"
[2019-10-31T10:57:28Z DEBUG fuse::request] GETATTR(510) ino 0x0000000000000001
```

## まとめ
今回はファイル移動を実装しました。  
次回はシンボリックリンク、ハードリンクを実装していきます。

# シンボリックリンク・ハードリンク
シンボリックリンクとハードリンクを作成できるようにします。
実装する関数は以下になります。

```rust
fn readlink(&mut self, _req: &Request<'_>, ino: u64, reply: ReplyData) {
    ...
}
fn symlink(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, link: &Path, reply: ReplyEntry) {
    ...
}
fn link(&mut self, _req: &Request<'_>, ino: u64, newparent: u64, newname: &OsStr, reply: ReplyEntry) {
    ...
}
```

## DB関数
追加したDB関数は以下になります。

```rust
/// ハードリンクを追加する。ディレクトリエントリを追加し、参照カウントを1増やす
fn link_dentry(&mut self, inode: u32, parent: u32, name: &str) -> Result<DBFileAttr>;
```


## link
```rust
fn link(&mut self, _req: &Request<'_>, ino: u64, newparent: u64, newname: &OsStr, reply: ReplyEntry);
```

引数 `ino` で対象のinode番号、 `newparent` で親ディレクトリのinode、 `newname` で名前が与えられるので、ハードリンクを作成します。

ハードリンクを作成することで、別のパスが全く同じファイルを指す事ができます。
inode番号が同じなので、ファイルパス以外のデータ、メタデータは同じになります。  
全てのハードリンクが削除されるまで、ファイルは削除されません。

ハードリンクはディレクトリに適用する事はできません。また、ディレクトリのハードリンクを作成することはできません。  
これらのチェックはカーネルがやっていて、 `link(2)` を実行した場合エラーになり、 `link` 関数が呼ばれません。

`link` も `lookup count` を1増やす事に注意してください。

```rust
fn link(&mut self, _req: &Request<'_>, ino: u64, newparent: u64, newname: &OsStr, reply: ReplyEntry) {
    // リンクの追加
    let attr = match self.db.link_dentry(ino as u32, newparent as u32, newname.to_str().unwrap()) {
        Ok(n) => n,
        Err(err) => match err.kind() {
            // 元のパスがディレクトリだった(カーネルがチェックしているので発生しないはず)
            ErrorKind::FsParm{description} => {reply.error(EPERM); debug!("{}", &description); return;},
            // リンク先にファイルまたはディレクトリがある(カーネルがチェックしているので発生しないはず)
            ErrorKind::FsFileExist{description} => {reply.error(EEXIST); debug!("{}", &description); return;},
            _ => {reply.error(ENOENT); debug!("{}", err); return;}
        }
    };
    reply.entry(&ONE_SEC, &attr.get_file_attr(), 0);
    // lookup countの追加
    let mut lc_list = self.lookup_count.lock().unwrap();
    let lc = lc_list.entry(ino as u32).or_insert(0);
    *lc += 1;
}
```

## symlink
```rust
fn symlink(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, link: &Path, reply: ReplyEntry);
```

引数の `link` で指定されたパスに対するシンボリックリンクを、
`parent` で指定されたinode番号のディレクトリ内に、 `name` という名前で作成します。

シンボリックリンクはリンク先のパスをデータに持つ特殊なファイルです。  
ファイルシステムはパスを保存するだけで、リンク先に対して特に操作を行う必要はありません。
リンク先にファイルやディレクトリが存在していなくてもOKです。

シンボリックリンクの内容が長すぎる場合、 `ENAMETOOLONG` を返すことができます。  
どの程度の長さでエラーにするかはファイルシステムが決めますが、あまり長くても(4096を超えるぐらい)カーネルが別のエラーを返してくるので、
1024～4096ぐらいの間に設定しておくといいです。

`symlink` もlookup count を増やす必要がある事に注意してください。

```rust
fn symlink(&mut self, req: &Request, parent: u64, name: &OsStr, link: &Path, reply: ReplyEntry) {
    let now = SystemTime::now();
    // メタデータの追加
    let mut attr = DBFileAttr {
        ino: 0,
        size: 0,
        blocks: 0,
        atime: now,
        mtime: now,
        ctime: now,
        crtime: now,
        kind: FileType::Symlink,
        perm: 0o777, // リンク自体のパーミッションは使われないので適当に設定する
        nlink: 0,
        uid: req.uid(),
        gid: req.gid(),
        rdev: 0,
        flags: 0
    };
    let ino = match self.db.add_inode(parent as u32, name.to_str().unwrap(), &attr) {
        Ok(n) => n,
        Err(err) => {reply.error(ENOENT); debug!("{}", err); return;}
    };
    let data = link.to_str().unwrap().as_bytes();
    let block_size = self.db.get_db_block_size() as usize;
    if data.len() > block_size {
        reply.error(ENAMETOOLONG);
        return;
    }
    // ファイルの内容(リンク先のパス)の書き込み
    match self.db.write_data(ino, 1, &data, data.len() as u32) {
        Ok(n) => n,
        Err(err) => {reply.error(ENOENT); debug!("{}", err); return;}
    }
    attr.ino = ino;
    reply.entry(&ONE_SEC, &attr.get_file_attr(), 0);
}
```

## readlink

```rust
fn readlink(&mut self, _req: &Request<'_>, ino: u64, reply: ReplyData);
```

引数の `ino` で指定されたシンボリックリンクの内容(シンボリックリンク先のパス) を返します。

```rust
fn readlink(&mut self, _req: &Request<'_>, ino: u64, reply: ReplyData) {
    let ino = ino as u32;
    let attr = match self.db.get_inode(ino) {
        Ok(n) => match n {
            Some(attr) => attr,
            None => {reply.error(ENOENT); return;}
        },
        Err(err) => {reply.error(ENOENT); debug!("{}", err); return;}
    };

    if attr.kind != FileType::Symlink {
        reply.error(EINVAL);
        return;
    }
    let size = attr.size;
    let mut data = match self.db.get_data(ino as u32, 1, size) {
        Ok(n) => n,
        Err(_err) => {reply.error(ENOENT); return; }
    };
    data.resize(size as usize, 0);
    reply.data(&data);
}
```

## 実行結果

### ハードリンクの作成

```text
$ ln hello.txt hello.hardlink
$ cat hello.hardlink
Hello world!
```

```text
[2019-11-05T11:12:56Z DEBUG fuse::request] LOOKUP(104) parent 0x0000000000000001, name "hello.txt"
[2019-11-05T11:12:56Z DEBUG fuse::request] LOOKUP(106) parent 0x0000000000000001, name "hello.hardlink"
[2019-11-05T11:12:56Z DEBUG fuse::request] LINK(108) ino 0x0000000000000002, newparent 0x0000000000000001, newname "hello.hardlink"
[2019-11-05T11:12:56Z DEBUG fuse::request] GETATTR(110) ino 0x0000000000000001
[2019-11-05T11:13:01Z DEBUG fuse::request] OPEN(120) ino 0x0000000000000002, flags 0x8000
[2019-11-05T11:13:01Z DEBUG fuse::request] READ(122) ino 0x0000000000000002, fh 0, offset 0, size 4096
[2019-11-05T11:13:01Z DEBUG fuse::request] RELEASE(124) ino 0x0000000000000002, fh 0, flags 0x8000, release flags 0x0, lock owner 0
[2019-11-05T11:13:01Z DEBUG fuse::request] GETATTR(126) ino 0x0000000000000001
```

### シンボリックリンクの作成

```text
$ ln -s hello.txt hello.symlink
$ cat hello.symlink
Hello world!
```

```text
[2019-11-05T11:15:55Z DEBUG fuse::request] LOOKUP(134) parent 0x0000000000000001, name "hello.symlink"
[2019-11-05T11:15:55Z DEBUG fuse::request] SYMLINK(136) parent 0x0000000000000001, name "hello.symlink", link "hello.txt"
[2019-11-05T11:15:55Z DEBUG fuse::request] GETATTR(138) ino 0x0000000000000001
[2019-11-05T11:16:01Z DEBUG fuse::request] LOOKUP(146) parent 0x0000000000000001, name "hello.symlink"
[2019-11-05T11:16:01Z DEBUG fuse::request] READLINK(148) ino 0x000000000000000a
[2019-11-05T11:16:01Z DEBUG fuse::request] LOOKUP(150) parent 0x0000000000000001, name "hello.txt"
[2019-11-05T11:16:01Z DEBUG fuse::request] OPEN(152) ino 0x0000000000000002, flags 0x8000
[2019-11-05T11:16:01Z DEBUG fuse::request] READ(154) ino 0x0000000000000002, fh 0, offset 0, size 4096
[2019-11-05T11:16:01Z DEBUG fuse::request] RELEASE(156) ino 0x0000000000000002, fh 0, flags 0x8000, release flags 0x0, lock owner 0
[2019-11-05T11:16:01Z DEBUG fuse::request] GETATTR(158) ino 0x0000000000000001
```

## まとめ
今回はシンボリックリンクとハードリンクの機能を作成しました。

# 後回しにしていた機能
## 概要
今まで基本的な操作を実装してきましたが、いくつかの機能を後回しにしていました。  
今回はそれらの機能を見ていきます。  
内容は「何らかの機能に対応する」や、「ルールを厳密に守る」ためのものですが、  
ファイルシステムの実装によっては必要がないものもあるので、必要に応じて実装していきます。

## マウントオプション
マウントオプションは `fuse::mount` の3番目の引数で指定します。  
各マウントオプションは、 `["-o", "some_option", "-o", "another_option"]` のように、頭に `"-o"` を付けます。

ファイルシステムからカーネルに渡すオプションの中で有用なものをいくつか挙げます。

### allow_other
デフォルトでは、FUSEを使ったファイルシステムはマウントしたユーザしかアクセスできません。  
マウントオプションに `allow_other` を指定することで、他のユーザもアクセスできるようになります。  
この機能を使うには、 `/etc/fuse.conf` に `user_allow_other` を書き込む必要があります。

また、rootのみアクセスを許可する `allow_root` もあります。

### default_permissions
デフォルトでは、FUSEはアクセス権のチェックを一切行わず、ファイルシステムがチェックする必要があります。  
マウントオプションに `default_permissions` を指定することで、アクセス権チェックを全てカーネルに任せることができます。  
カーネルは、ファイルオーナー、グループ、ファイルのモードから、ユーザがアクセスできるかをチェックします。

## open/close
今までファイルやディレクトリのオープンについては一切無視してきました。  
ここで、ファイルを開く/閉じる関数を実装していきます。

## open
```rust
fn open(&mut self, _req: &Request<'_>, ino: u64, flags: u32, reply: ReplyOpen) {
    reply.opened(0, 0);
}
```
引数の `ino` で指定されたinode番号のファイルを、 `flags` で指定されたフラグで開きます。

### 引数のフラグ
フラグは `_flags` 引数で渡されます。
ただし、 `open` では `O_CREAT, O_EXCL, O_NOCTTY` の3つはカーネルで省かれるので、ファイルシステムは検知できません。

マウントオプションで `-o default_permissions` が指定されている場合を除いて、
ファイルシステムは、アクセスモードのフラグを使ってアクセス権チェックを行わないといけません。  
`O_RDONLY`, `O_WRONLY`, `O_RDWR` のフラグに応じて、 `read` `write` できるか決定します。

ライトバックキャッシュが有効の時、カーネルは `O_WRONLY` でもreadしてくる事があるので、
マウントオプションで `-o writeback` が有効の場合は読めるようにしておく必要があります。  
[libfuseのサンプルの修正例](https://github.com/libfuse/libfuse/commit/b3109e71faf2713402f70d226617352815f6c72e)
を参考にしてください。

また、 `write` の時に説明した `O_APPEND` フラグにも注意してください。

ここに書かれていないフラグも、 `open(2)` の定義通りの動作が要求されます。  
また、状況に応じて `create` が呼ばれるので、そちらでも対処する必要があります。  
渡されるフラグは以下の通りです。

|flag|説明|
|---|---|
|O_APPEND|書き込みは全て追記する|
|O_ASYNC|入出力可能になったとき、シグナルSIGIOを返す(ソケットまたはパイプの時)|
|O_DIRECT|ファイルシステムはキャッシュを最小限にする|
|O_DSYNC|writeの度に、データとデータの読み書きに必要なメタデータ(サイズなど)をディスクに書き込む|
|O_LARGEFILE|64ビットのサイズを持つファイルをオープン可能にする|
|O_NOATIME|read時にatimeを更新しない|
|O_NONBLOCK|処理を完全にブロックしない(ある程度でタイムアウトする)|
|O_SYNC|writeの度に、データと全てのメタデータをディスクに書き込む|

以下は対処不要なフラグです。

|flag|説明|
|---|---|
|O_CREAT|状況に応じてcreateとopenに割り振られる|
|O_DIRECTORY|opendirが呼ばれる|
|O_EXCL|カーネルでチェックしてくれている|
|O_NOFOLLOW|カーネルでチェックしてくれている|
|O_TRUNC|setattrが呼ばれる|

### reply
openのreplyは、以下のようにして操作します。

```rust
reply.opened(fh, flags);
```

`fh` はファイルハンドル、 `flags` はフラグです。

#### ファイルハンドル
ファイルシステムはファイルハンドル `fh: u64` を戻り値に含めることができます。( `reply.opened()` の1番目の引数)。

`open(2)` の実行結果として `3` のようなファイルディスクリプタ(fd)が返ってきますが、
カーネルがこのfdとfhの対応を覚えていて、同じfdに対する `write` や `read` の引数として、このfhを入力してくれます。  
fhは自由に決めることができ、ポインタ、リストのインデックス、その他好きな値をファイルシステム側で定める事ができます。  

#### フラグ
ファイルシステムはフラグを戻り値に含めることができます( `reply.opened()` の2番目の引数)。  
`fuse-abi` の `FOPEN_DIRECT_IO` `FOPEN_KEEP_CACHE` `FOPEN_NONSEEKABLE` `FOPEN_PURGE_ATTR` `FOPEN_PURGE_UBC`
が相当します。(それぞれビットマスク)

通常はあまり使わないと思われるので、詳しく知りたい場合は [libfuseのfuse_common.h](https://github.com/libfuse/libfuse/blob/master/include/fuse_common.h)
を参照してください。

## flush
```rust
fn flush(&mut self, _req: &Request<'_>, ino: u64, fh: u64, lock_owner: u64, reply: ReplyEmpty) {
    ...
}
```

`close(2)` システムコールの度に呼ばれます。

同じくファイルを閉じるための関数である `release` は値を返さないので、
`close(2)` に対してエラーを返したい場合はここで行う必要があります。

`dup, dup2, fork` によりプロセスが複製される事で、一つの `open` に対して、複数の `flush` が呼ばれる場合があり、  
どれが最後の `flush` なのか識別するのは不可能なので、後で(または `flush` 処理中に) 別の `flush` が呼ばれてもいいように対応します。  
どうしても1度しか実行してはいけない処理がある場合は、 `release` で行います。

例えば、sshfsでは、`flush` でスレッド間でロックをかけて、競合しないように書き込み処理の後始末を行っています。

`flush` という名前が付いてはいますが、 `fsync` のようにデータをディスクに書き込む事を義務付けられてはいません。  
`close(2)` 時にデータが書き込まれているかどうかは使用者側の責任になります。

`setlk` `getlk` のようなファイルロック機構をファイルシステムが実装している場合、引数の `lock_owner` が持つロックを全て開放します。

## relase
```rust
fn release(&mut self, _req: &Request<'_>, _ino: u64, _fh: u64, _flags: u32, _lock_owner: u64, _flush: bool, reply: ReplyEmpty) {
    ...
    reply.ok();
}
```

ファイルを閉じます。  
一つの `open` に対して、必ず一つの `release` が呼ばれます。

ファイルシステムはエラーを返してもよいですが、呼び出し元の `close(2)` には値が渡らないので、特に意味はありません。

引数の `fh` は `open` 時にファイルシステムが指定した値で、 `flags` は `open` 時の引数 `flag` と同一の値になります。

## opendir
```rust
fn opendir(&mut self, _req: &Request<'_>, ino: u64, flags: u32, reply: ReplyOpen) {
    ...
    reply.opened(0, 0);
}
```

引数の `ino` で指定されたinode番号のディレクトリを開きます。

ファイル同様、 `fh: u64` を戻り値に含めることができます( `reply.opened()` の1番目の引数)。  
`fh` は `readdir` および `releasedir` の引数としてカーネルから渡されます。  
`fh` に何も入れないことも可能ですが、`opendir` から `relasedir` までの間に
ディレクトリエントリが追加または削除された場合でも `readdir` の整合性を保つために、何らかの情報を入れておく事が推奨されます。  
open中に追加または削除されたエントリは返しても返さなくても良いですが、追加または削除されていないエントリは必ず返さないといけないので、
厳密に対応しようとするなら、ディレクトリエントリをコピーして、情報を保持しておくとよいです。

引数の `flags` は `open(2)` で `O_DIRECTORY` を選択した時に `opendir` が呼ばれるので、その引数です。  
手元の環境で `opendir(3)` を実行すると、 `O_NONBLOCK`, `O_DIRECTORY`, `O_LARGEFILE` が呼ばれました。

## releasedir
```rust
fn releasedir(&mut self, _req: &Request<'_>, _ino: u64, _fh: u64, _flags: u32, reply: ReplyEmpty) {
    ...
    reply.ok();
}
```

`opendir` で確保したリソースを解放します。  
引数の `fh` は `opendir` でファイルシステムが渡した値です。

一度の `opendir` に対して、一度だけ `releasedir` が呼び出されます。

## fsync
```rust
fn fsync(&mut self, _req: &Request<'_>, ino: u64, fh: u64, datasync: bool, reply: ReplyEmpty) {
    ...
}
```

引数の `ino` で指定されたinode番号のファイルのキャッシュを永続領域に書き込みます。  
引数の `fh` は `open` でファイルシステムが渡した値です。

fsyncが呼ばれるまでは、書き込まれたデータやメタデータはキャッシュしていてよいです。  
つまり、なんらかの事情(kill, マシンの電源断)でファイルシステムのデーモンが即座に終了したとしても、データの保証はしません。

一方、fsyncに対して `reply.ok()` を返した時点で、
データがディスクやネットワークの先などどこかの領域に保存されている事を保証しなければなりません。

引数 `datasync` が `true` である場合、メタデータは書き込みません。

## fsyncdir
```rust
fn fsyncdir (&mut self, _req: &Request<'_>, ino: u64, fh: u64, datasync: bool, reply: ReplyEmpty) {
    ...
}
```

引数の `ino` で指定されたディレクトリ内のデータ(ディレクトリエントリ)をディスクに書き込みます。
引数の `datasync` が `true` の時、ディレクトリ自体のメタデータは更新しません。

この関数が呼ばれるまでは、ファイル作成時などに作成されるディレクトリエントリはディスクに書き込まれることを保証しません。  
`fsync(2)` でディレクトリを引数に取った時に呼ばれます。

## statfs
```rust
fn statfs(&mut self, _req: &Request<'_>, _ino: u64, reply: ReplyStatfs) {
    ...
    reply.statfs(blocks, bfree, bavail, files, ffree, bsize, namelen, frsize);
}
```

`statfs(2)` で使うファイルシステムの情報を返します。
`df` コマンド等で出てくる奴です。

replyの引数の意味は次のようになります。

```
blocks: u64; // frsize単位でのファイルシステムの総ブロック数 (ex: 1024)
bfree: u64; // ファイルシステムの空きブロック数
bavail: u64; // スーパーユーザ用に予約されている領域を除いた空きブロック数
files: u64; // 総inode数
ffree: u64; // 空きinode数
bsize: u32; // 推奨されるI/Oで使用するブロックのバイト数 (ex: 4096)
namelen: u32; //ファイル名の最大長
frsize: u32; //最小のブロックのバイト数 (ex: 512, 1024, 2048, 4096)
```

# 拡張ファイル属性
## 概要
拡張ファイル属性は、ユーザがkey-valueペアのメタデータを自由にファイルやディレクトリに付ける事が出来る機能です。  
Linuxでは主にACLやselinuxが利用しています。

拡張ファイル属性の操作に必要な関数は以下の4つです。

```rust
fn setxattr(&mut self, _req: &Request<'_>, _ino: u64, _name: &OsStr, _value: &[u8], _flags: u32, _position: u32, reply: ReplyEmpty) {
    ...
}
fn getxattr(&mut self, _req: &Request<'_>, _ino: u64, _name: &OsStr, _size: u32, reply: ReplyXattr) {
    ...
}
fn listxattr(&mut self, _req: &Request<'_>, _ino: u64, _size: u32, reply: ReplyXattr) {
    ...
}
fn removexattr(&mut self, _req: &Request<'_>, _ino: u64, _name: &OsStr, reply: ReplyEmpty) {
    ...
}
```

## setxattr
```rust
fn setxattr(&mut self, _req: &Request<'_>, ino: u64, name: &OsStr, value: &[u8], flags: u32, position: u32, reply: ReplyEmpty);
```
拡張ファイル属性を設定します。
引数は `setxattr(2)` と同様です。

実装しない( `ENOSYS` を返す) 場合、拡張ファイル属性をサポートしない( `ENOTSUP` と同様)と解釈され、
以降はカーネルからファイルシステムの呼び出しを行わずに失敗するようになります。

拡張属性は `name: value` の形式で与えられます。 

引数の `position` はmacのリソースフォークで使用されている値で、
基本は0です。osxfuseにのみ存在する引数です。(現在のrustの実装では、mac以外は0を返す)  
fuse-rsでは、 `getxattr` の方に [実装されていないまま](https://github.com/zargony/fuse-rs/issues/40) なので、
とりあえず放置でよいと思われます。

引数の `flags` には `XATTR_CREATE` または `XATTR_REPLACE` が指定されます。  
`XATTR_CREATE` が指定された場合、既に属性が存在する場合は `EEXIST` を返して失敗します。  
`XATTR_REPLACE` が指定された場合、属性が存在しない場合 `ENODATA` を返して失敗します。  
デフォルトでは、属性が存在しない場合は拡張ファイル属性を作成し、既に存在する場合は値を置き換えます。

実装は以下のようになります。

```rust
fn setxattr(&mut self, _req: &Request<'_>, ino: u64, name: &OsStr, value: &[u8], flags: u32, _position: u32, reply: ReplyEmpty) {
    let ino = ino as u32;
    let name = name.to_str().unwrap();
    // フラグチェック
    if flags & XATTR_CREATE as u32 > 0 || flags & XATTR_REPLACE as u32 > 0 {
        match self.db.get_xattr(ino, name) {
            Ok(_) => {
                if flags & XATTR_CREATE as u32 > 0 {
                    reply.error(EEXIST);
                    return;
                }
            },
            Err(err) => {
                match err.kind() {
                    ErrorKind::FsNoEnt {description: _} => {
                        if flags & XATTR_REPLACE as u32 > 0 {
                            reply.error(ENODATA);
                            return;
                        }
                    },
                    _ => {
                        reply.error(ENOENT);
                        return;
                    }
                }
            }
        };
    }
    match self.db.set_xattr(ino, name, value) {
        Ok(n) => n,
        Err(err) => {reply.error(ENOENT); debug!("{}", err); return;}
    };
    reply.ok();
}
```

## getxattr
```rust
fn getxattr(&mut self, _req: &Request<'_>, ino: u64, name: &OsStr, size: u32, reply: ReplyXattr);
```

引数の`ino` で指定されたinode番号のファイルの、 `name` で指定された拡張ファイル属性の値を取得します。

引数の `size` が0の場合、値のデータのバイト数を `reply.size()` に入れます。

`size` が0でない場合、以下のような処理になります。  
値のデータのバイト数が `size` 以下の場合、 `reply.data()` に値を入れて返します。  
`size` を超える場合、 `reply.error(ERANGE)` を返します。

```rust
fn getxattr(&mut self, _req: &Request<'_>, ino: u64, name: &OsStr, size: u32, reply: ReplyXattr) {
    let ino = ino as u32;
    let name = name.to_str().unwrap();
    let value = match self.db.get_xattr(ino, name) {
        Ok(n) => n,
        Err(err) => {reply.error(ENOENT); debug!("{}", err); return;}
    };
    if size == 0 {
        reply.size(value.len() as u32);
    } else if size < value.len() as u32 {
        reply.error(ERANGE);
    } else {
        reply.data(value.as_slice());
    }
}
```

## listxattr
```rust
fn listxattr(&mut self, _req: &Request<'_>, _ino: u64, _size: u32, reply: ReplyXattr);
```

引数の `ino` で指定されたinode番号のファイルにセットされている拡張ファイル属性の名前一覧を返します。

返すべきデータは、「ヌル終端された文字列が連続して並んでいる」フォーマットになります。  
例えば、 `user.xxx.data` と `user.yyy.name` という名前の拡張ファイル属性がある場合、データは以下のようになります。  
ex: `user.xxx.data\0user.yyy.name\0`

引数の `size` が0の場合、連結した文字列のサイズ(末尾の `\0` も含める)を `reply.size()` に入れます。

0でない場合、データのサイズが `size` 以下の場合、 `reply.data()` にデータを入れて返します。  
`size` を超える場合、 `reply.error(ERANGE)` を返します。

```rust
fn listxattr(&mut self, _req: &Request<'_>, ino: u64, size: u32, reply: ReplyXattr) {
    let ino = ino as u32;
    let names =  match self.db.list_xattr(ino) {
        Ok(n) => n,
        Err(err) => {reply.error(ENOENT); debug!("{}", err); return;}
    };
    let mut data: Vec<u8> = Vec::new();
    for v in names {
        data.extend(v.bytes());
        data.push(0);
    }
    if size == 0 {
        reply.size(data.len() as u32);
    } else if size < data.len() as u32 {
        reply.error(ERANGE);
    } else {
        reply.data(data.as_slice());
    }
}
```

## removexattr
```rust
fn removexattr(&mut self, _req: &Request<'_>, _ino: u64, _name: &OsStr, reply: ReplyEmpty);
```

引数の`ino` で指定されたinode番号のファイルの、 `name` で指定された拡張ファイル属性を削除します。

```rust
fn removexattr(&mut self, _req: &Request<'_>, ino: u64, name: &OsStr, reply: ReplyEmpty) {
    let ino = ino as u32;
    let name = name.to_str().unwrap();
    match self.db.delete_xattr(ino, name) {
        Ok(n) => n,
        Err(err) => {reply.error(ENOENT); debug!("{}", err); return;}
    };
    reply.ok();
}
```

# アクセス権のチェック
## 概要
`allow_other` をマウントオプションで指定した場合、マウントしたユーザ以外がアクセスできるようになるため、
アクセス権のチェックが必要になってきます。  
マウントオプションで `default_permissions` を指定すればカーネルがチェックしてくれるので特に何かする必要はありませんが、
何かしら独自の機構を用意したい場合、チェックが必要な関数毎にチェックする必要があります。

## access
```rust
fn access(&mut self, _req: &Request<'_>, _ino: u64, _mask: u32, reply: ReplyEmpty) {
    ...
}
```

プロセスの実行ユーザが、引数の `ino` で

# ロック機構
## 概要
fuseではカーネルがロックのチェックをしてくれています。  
ただし、ネットワークでファイルを共有しているなどの理由で独自のロック機構が必要な場合は、自分で実装する必要があります。
