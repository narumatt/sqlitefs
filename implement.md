# 概要

Filesystem in Userspace(FUSE) はユーザ空間でファイルシステムを実現する仕組みです。

一般的にファイルシステムを作るというと、カーネルモジュールを作成しなければならないので、いろいろと苦労が多いですが、FUSEを使えば大分楽に実装できます。  
また、HDDなどの実デバイスに直接読み書きするだけでなく、仮想的なファイルシステムを作るのにも都合がよいです。

そんな訳で、FUSEを使ったSSH as a filesystem や AWS S3 as a filesystemといった
「読み書きできる何かをファイルシステムとしてマウント出来るようにするソフトウェア」があれこれと存在します。

ただし、カーネルモジュールを作るより楽とはいえ、FUSEを使ったソフトウェアの作成には困難が伴います。  
ある程度ファイルシステムの知識は必要ですし、チュートリアルはほどほどの所で終わってしまい、「あとはsshfsの実装などを見てくれ！」とコードの海に投げ出されます。

本書は、RustによるFUSEインターフェースの実装である `rust-fuse` を用いてFUSEを使ったファイルシステムの実装に挑戦し、
気をつけるべき点などを記録したものです。

## FUSEの仕組み(アバウト)

FUSE本体はLinuxカーネルに付属するカーネルモジュールであり、大抵のディストリビューションではデフォルトでビルドされてインストールされます。

FUSEがマウントされたディレクトリ内のパスに対してシステムコールが呼ばれると、以下のように情報がやりとりされます。

```
システムコール <-> VFS <-> FUSE <-> FUSEインターフェース <-> 自分のプログラム
```

[Wikipediaの図](https://ja.wikipedia.org/wiki/Filesystem_in_Userspace) を見ると分かりやすいです。

## FUSEインターフェース

FUSEはデバイス `/dev/fuse` を持ち、ここを通じてユーザ空間とやりとりを行います。  
前項の `FUSE <-> FUSEインターフェース` の部分です。

規定のプロトコルを用いて `/dev/fuse` に対してデータを渡したり受け取ったりするのがFUSEインターフェースです。  
有名な実装として、 [libfuse](https://github.com/libfuse/libfuse) があります。  
このlibfuseが大変強力なので、大抵の言語でのFUSEインターフェースはlibfuseのラッパーになっています。

## rust-fuse
Rustには独自のFUSEインターフェースの実装 `Rust FUSE(rust-fuse)` があります。ありがたいですね。  
元々プロトコルが同じなので、インターフェースの関数はlibfuseと大変似ています。そのため、何か困った時にはlibfuseの情報が流用できたりします。ありがたいですね。

現時点(2019/10) の最新版は0.3.1で、2年ぐらい更新されていませんが、次バージョン(0.4.0)が開発中です。  
0.3.1と0.4.0では日時関係の型が大幅に違うので注意してください。

# データの保存先
HDDの代わりになるデータの保存先を決めます。
今回はsqliteを使用します。  
sqliteは可変長のバイナリデータを持てるので、そこにデータを書き込みます。
DBなので、メタデータの読み書きも割と簡単にできるでしょう。

## データベース構造
テーブルはメタデータテーブル(MDT)とディレクトリエントリテーブル(DET)とブロックデータテーブル(BDT)3つに分けます。  
今後拡張ファイル属性が必要になってきた場合、拡張属性データテーブル(XATTRT)を追加します。

以下では各テーブルについてざっと説明していきます。

## MDT
メタデータは一般的なファイルシステムのメタデータと大体同じ形式です。  
rust-fuseが関数で渡したり要求したりするメタデータ構造体は以下のようになっています。

```
pub struct FileAttr {
    /// Inode number
    pub ino: u64,
    /// Size in bytes
    pub size: u64,
    /// Size in blocks. *Sparse File に対応する場合、実際に使用しているブロック数を返す
    pub blocks: u64,
    /// Time of last access. *read(2)実行時に更新される
    pub atime: Timespec,
    /// Time of last modification. *write(2)またはtruncate(2)実行時に更新される
    pub mtime: Timespec,
    /// Time of last change. *メタデータ変更時に更新される。 write(2)またはtruncate(2)でファイル内容が変わるときも更新される
    pub ctime: Timespec,
    /// Time of creation (macOS only)
    pub crtime: Timespec,
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
|kind|int|ファイル種別(ファイル:0o0100000, ディレクトリ:0o0040000とする)
|mode|int|パーミッション(ファイル種別含む)|
|nlink|int|ハードリンク数|
|uid|int|uid|
|gid|int|gid|
|rdev|int|デバイスタイプ|
|flags|int|フラグ(mac用)|

idをinteger primary keyにします。これがinode番号になります。

kindはファイル種別です。 FUSEでは `stat(2)` 同様modeにファイル種別のビットも含まれていて、cのlibfuseでは `libc::S_IFREG` 等を用いて判別する必要がありますが、  
rust-fuseではライブラリ側で上手いこと処理してくれています。

## BDT
ブロックデータテーブル(BDT)のblobにデータを格納します。
BDTはファイルのinode, 何番目のブロックか、の列を持ちます。具体的には以下のようになります。

|列名 | 型 | 概要|
|---|---|---|
|file_id|int|ファイルのinode番号 (pkey)(foreign key)|
|block_num|int|データのブロック番号(1始まり)(pkey)|
|data|blob|データ(4kByte単位とする)|

外部キー `foreign key (file_id) references metadata(id) on delete cascade`
を指定する事で、ファイルのメタデータが消えたらデータも削除されるようにします。

「あるファイルのあるブロック」は一意なので、主キーとして `(file_id, block_num)` を指定します。

## DET
ディレクトリ構造を表現する方法は、以下の2つの候補があります。

1. 分散ファイルシステムでよくある、フルパスを各ファイルが持っていて、文字列操作で各ディレクトリの情報を得る方法
1. 一般的なファイルシステムのように、ディレクトリエントリを作る方法

今回は実装の楽そうな後者のディレクトリエントリ方式で行います。

必要そうなのは以下のデータ

|列名 | 型 | 概要|
|---|---|---|
|parent_id|int|親ディレクトリのinode (pkey)(foreign key)|
|child_id|int|子ファイル/子ディレクトリのinode (foreign key)|
|file_type|int|ファイルタイプ|
|name|text|ファイル/ディレクトリ名 (pkey)|

あらゆるディレクトリは `.` と `..` のエントリを持ちます。(ルートの `..` は `.` です。)  
`.` と `..` は返さなくともよい事になっていますが、その場合は呼び出し側の責任で処理する事になります。

ファイルタイプはメタデータとディレクトリエントリで2重に持っていますが、同じinodeに対してファイルタイプが変わることは無いのでよしとします。

## SQL
テーブル作成SQLは次のようになります。

```
PRAGMA foreign_keys=ON;
BEGIN TRANSACTION;
CREATE TABLE metadata(
            id integer primary key,
            size int default 0 not null,
            atime text,
            atime_nsec int,
            mtime text,
            mtime_nsec int,
            ctime text,
            ctime_nsec int,
            crtime text,
            crtime_nsec int,
            kind int,
            mode int,
            nlink int default 0 not null,
            uid int default 0,
            gid int default 0,
            rdev int default 0,
            flags int default 0
            );
INSERT INTO metadata VALUES(1,0,'2019-10-21 05:19:50',991989258,'2019-10-21 05:19:50',991989258,'2019-10-21 05:19:50',991989258,'2019-10-21 05:19:50',991989258,16384,16832,1,0,0,0,0);
CREATE TABLE data(
            file_id int,
            block_num int,
            data blob,
            foreign key (file_id) references metadata(id) on delete cascade,
            primary key (file_id, block_num)
            );
CREATE TABLE dentry(
            parent_id int,
            child_id int,
            file_type int,
            name text,
            foreign key (parent_id) references metadata(id) on delete cascade,
            foreign key (child_id) references metadata(id) on delete cascade,
            primary key (parent_id, name)
            );
INSERT INTO dentry VALUES(1,1,16384,'.');
INSERT INTO dentry VALUES(1,1,16384,'..');
CREATE TABLE xattr(
            file_id int,
            name text,
            value blob,
            foreign key (file_id) references metadata(id) on delete cascade,
            primary key (file_id, name)
            );
COMMIT;
```

初期データとして、ルートディレクトリの情報を入れています。  
FUSEでは、ルートディレクトリのinodeは1です。

# Hello!
## 概要
第一段階として、rust-fuseに付属する、サンプルプログラムの `HelloFS` と同じ機能を実装すします。
`HelloFS` は以下の機能があります。

1. ファイルシステムはリードオンリー
1. ルート直下に `hello.txt` というファイルがあり、 `"Hello World!\n"` というデータが読み込める

必要なのは以下の4つの関数です。

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

open/closeする関数を実装せずにread関数やreaddir関数を実装していますが、今回のようにreadonlyで状態を持たないファイルシステムの場合、デフォルトの実装で動作します。  
これらの関数については今後実装する必要が出てきた時に説明します。


## DB関数
データベースを読み書きする関数です。  
今回作成した関数は以下の通り。

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

## fuseの関数全般の話
### fuseの関数
ファイルシステムなので、関数は基本的に受け身です。システムコールに応じて呼び出されます。  
`Filesystem` トレイトが定義されているので、必要な関数を適宜実装していきます。

### 戻り値
各関数に戻り値は存在せず、 `reply` 引数を操作して、呼び出し元に値を受け渡します。

`reply.ok()` `reply.error(ENOSYS)` `reply.attr(...)` 等が使えます。

## lookup
親ディレクトリのinode、当該ディレクトリ/ファイルの名前が与えられるので、ディレクトリエントリとメタデータを返します。  
lookup実行時には `lookup count` をファイルシステム側で用意して、増やしたりしなければなりませんが、今回はreadonlyのファイルシステムなので無視します。  
`lookup count` についてはunlink実装時に説明します。

必要なデータは以下の通り。

```
    //正常
    reply.entry(&TTL, &ATTR, &GENERATION);
    エラー
    reply.error(ENOENT);
```

`reply.entry()` の3つの引数について説明します。

- TTL

`time::Timespec` で期間を指定します。  
TTLの間はカーネルは再度問い合わせに来ません。

- ATTR

対象のメタデータ。 `fuse::FileAttr` を返します。

- generation

inodeの世代情報を `u64` で返します。削除されたinodeに別のファイルを割り当てた場合、
前のファイルと違うファイルである事を示すために、generationに別の値を割り当てます。  
ただし、この値をチェックするのは(知られているものでは)nfsしかないです。  
今回はinodeの使い回しが無いので、常時 `0` に設定します。

[libfuseの説明](https://libfuse.github.io/doxygen/structfuse__entry__param.html#a4c673ec62c76f7d63d326407beb1b463)
も参考にしてください。

- ENOENT

対象のディレクトリエントリが存在しない場合、 `reply.error(ENOENT)` でエラーを返します。

実装は以下のようになります。

```
fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
    match self.db.lookup(parent as u32, name.to_str().unwrap()) {
        Ok(n) => {
            reply.entry(&Timespec{sec: 1, nsec: 0}, &n.get_file_attr() , 0);
        },
        Err(_err) => reply.error(ENOENT)
    };
}
```

## getattr
引数のinodeで指定されたファイルのメタデータを返します。
内容については `lookup` で返す `ATTR` と同じです。

```
fn getattr(&mut self, _req: &Request, ino: u64, reply: ReplyAttr) {
    match self.db.get_inode(ino as u32) {
        Ok(n) => {
            reply.attr(&ONE_SEC, &n.get_file_attr());
            debug!("filesystem:getattr, return:{:?}", n.get_file_attr());
        },
        Err(_err) => reply.error(ENOENT)
    };
}
```

## read
引数のinodeで指定されたファイルをoffsetバイト目からsizeバイト分読み込みます。  
読み込んだデータは `reply.data(&data)` を実行して返します。

ファイルの読み込む位置を指定する方法は色々とありますが、fuseは `pread(2)` 相当の関数を一つ実装するだけで済むようにしてくれています。

EOFまたはエラーを返す場合を除いて、readはsizeで指定されたサイズのデータを返さないといけません。実データが足りなくて返せない場合は0埋めします。  
例えば、長さ200byteのデータに対して、4096byteの要求が来ることがあるので、3896byte分を0埋めして返さなければなりません。  
例外として、`direct_io` フラグを `open` の戻り値として指定した場合、カーネルは `read(2)` システムコールの戻り値として、
ファイルシステムの戻り値を直接使うので、sizeより小さいデータを返してもよいです。

引数の `fh` は `open` 時にファイルシステムが指定した値です。今回は `open` を実装していないので0です。

```
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
        // 要求より戻ってきたデータのサイズが小さかった場合の対応
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
指定されたinodeのディレクトリのディレクトリエントリを返します。 `ls` コマンドの結果を返すイメージです。  
一定サイズのバッファが渡されるので、一杯になるまでディレクトリエントリを入れて返します。

引数の `fh` は `opendir` でファイルシステムが渡した値です。今回は `opendir` を実装していないので0です。

cでは `fuse_add_direntry()` という関数を使用してバッファを埋めますが、rustでは引数で渡された `reply: ReplyDirectory` を使用します。  
具体的には以下のように使います。

```
result = reply.add(target_inode, offset, FileType.RegularFile, filename);
```

`reply.add()` でデータを追加していき、最終的に `reply.ok()` を実行すると、データが返せます。

バッファが一杯の時、 `ReplyDirectory.add()` は `true` を返します。

`reply.add()` の引数の `offset` はファイルシステムが任意に決めたオフセットです。  
大抵はディレクトリエントリ一覧内のインデックスや次のエントリへのポインタが使われます。
同じディレクトリエントリ内で `offset` は一意でなければなりません。また、offsetは決まった順番を持たなければなりません。  
カーネルが`readdir`の 引数として `offset` に0でない値を指定してきた場合、
該当の `offset` を持つディレクトリエントリの、次のディレクトリエントリを返さなければならないからです。  
`readdir` の引数に `0` が来た場合「最初のディレクトリエントリ」を返さないといけないので、ファイルシステムは `offset` に0を入れてはならないです。

`.` と `..` は返さなくともよいですが、返さなかった場合の処理は呼び出し側のプログラムに依存します。

```
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

rust-fuseは [env_logger](https://github.com/sebasmagri/env_logger/)に対応しているので、最初に有効にしておきます。  
`DEBUG` レベルにすると各関数の呼び出しを記録してくれます。

```
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

## hello用の初期データ登録
以下のSQLを実行して、 `hello.txt` をファイルシステムに入れます。

```sql
PRAGMA foreign_keys=ON;
BEGIN TRANSACTION;
INSERT INTO metadata VALUES(2,13,'1970-01-01 00:00:00',0,'1970-01-01 00:00:00',0,'1970-01-01 00:00:00',0,'1970-01-01 00:00:00',0,32768,33188,1,0,0,0,0);
INSERT INTO data VALUES(2,1,X'48656c6c6f20576f726c64210a');
INSERT INTO dentry VALUES(1,2,32768,'hello.txt');
COMMIT;
```

## ビルド及び実行
`[プログラム名] [マウント先] [データベースファイル名]` で実行できます。

```
$ ./sqlite-fs ~/mount ~/filesystem.sqlite &
$ ls ~/mount
hello.txt
$ cat ~/mount/hello.txt
Hello World!
```

また、 `$ RUST_LOG=debug cargo run ~/mount` でビルドと実行( `~/mount` にマウントして、デバッグログを出力)ができます。  
試しに `cat ~/mount/hello.txt` を実行すると、以下のようなログが出力されます。

```
[2019-10-25T10:43:27Z DEBUG fuse::request] INIT(2)   kernel: ABI 7.31, flags 0x3fffffb, max readahead 131072
[2019-10-25T10:43:27Z DEBUG fuse::request] INIT(2) response: ABI 7.8, flags 0x1, max readahead 131072, max write 16777216
[2019-10-25T10:43:42Z DEBUG fuse::request] LOOKUP(4) parent 0x0000000000000001, name "hello.txt"
[2019-10-25T10:43:42Z DEBUG fuse::request] OPEN(6) ino 0x0000000000000002, flags 0x8000
[2019-10-25T10:43:42Z DEBUG fuse::request] READ(8) ino 0x0000000000000002, fh 0, offset 0, size 4096
[2019-10-25T10:43:42Z DEBUG fuse::request] FLUSH(10) ino 0x0000000000000002, fh 0, lock owner 12734418937618606797
[2019-10-25T10:43:42Z DEBUG fuse::request] RELEASE(12) ino 0x0000000000000002, fh 0, flags 0x8000, release flags 0x0, lock owner 0
```

ファイルシステムは `fusermount -u [マウント先]` でアンマウントできます。アンマウントするとプログラムは終了します。  
`Ctrl + c` 等でプログラムを終了した場合でもマウントしたままになっているので、かならず `fusermount` を実行してください。
