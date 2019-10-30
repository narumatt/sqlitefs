# 概要

Filesystem in Userspace(FUSE) はLinuxのユーザ空間でファイルシステムを実現する仕組みです。

一般的にファイルシステムを作るというと、カーネルモジュールを作成しなければならないのでいろいろと苦労が多いですが、FUSEを使えば大分楽に実装できます。  
また、HDDなどの実デバイスに直接読み書きするだけでなく、仮想的なファイルシステムを作るのにも都合がよいです。

そんな訳で、FUSEを使ったSSH as a filesystem や AWS S3 as a filesystemといった
「読み書きできる何かをファイルシステムとしてマウント出来るようにするソフトウェア」があれこれと存在します。

ただし、カーネルモジュールを作るより楽とはいえ、FUSEを使ったソフトウェアを作成するのは大変です。  
ある程度ファイルシステムの知識は必要ですし、チュートリアルを見てもほどほどの所で終わってしまい、「あとはsshfsの実装などを見てくれ！」とコードの海に投げ出されます。

本書は、RustによるFUSEインターフェースの実装である `rust-fuse` を用いてFUSEを使ったファイルシステムの実装に挑戦し、
得られた知見などを記録したものです。

## FUSEの仕組み(アバウト)

FUSE本体はLinuxカーネルに付属するカーネルモジュールで、大抵のディストリビューションではデフォルトで有効になっています。

FUSEを使ったファイルシステムがマウントされたディレクトリ内に対してシステムコールが呼ばれると、以下のように情報がやりとりされます。

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
Rustには(ほぼ)独自のFUSEインターフェースの実装 `Rust FUSE(rust-fuse)` があります。ありがたいですね。  
プロトコルが同じなので、インターフェースの関数はlibfuseのlowlevel関数と大変似ています。そのため、何か困った時にはlibfuseの情報が流用できたりします。ありがたいですね。

現時点(2019/10) の最新版は0.3.1で、2年ぐらい更新されていませんが、次バージョン(0.4.0)が開発中です。  
0.3.1と0.4.0では日時関係の型が大幅に違うので注意してください。

libfuseはマルチスレッドで動作し、並列I/Oに対応していますが、rust-fuseはシングルスレッドのようです。

# データの保存先
今回自分でファイルシステムを実装していく上で、HDDの代わりになるデータの保存先としてsqliteを使用します。

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
|kind|int|ファイル種別|
|mode|int|パーミッション(ファイル種別含む)|
|nlink|int|ハードリンク数|
|uid|int|uid|
|gid|int|gid|
|rdev|int|デバイスタイプ|
|flags|int|フラグ(mac用)|

idをinteger primary keyにします。これがinode番号になります。

kindはファイル種別です。 FUSEでは `stat(2)` 同様modeにファイル種別のビットも含まれていて、
cのlibfuseでは `libc::S_IFMT` (該当ビットのマスク) `libc::S_IFREG` (通常ファイルを示すビット) 等を用いて
`if((mode & S_IFMT) == S_IFREG)` のようにして判別する事ができます。

ファイル種別が問題になるのはメタデータを返す時ですが、rust-fuseではenumでファイル種別を定義しています。
`mknod` の引数で `mode` が生の値で渡ってくるので、 `mknod` を実装する場合のみ気をつける必要があります。

## BDT
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

## DET
ディレクトリ構造を表現する方法は、以下の2つの候補があります。

1. 分散ファイルシステムでよくある、フルパスを各ファイルが持っていて、文字列操作で各ディレクトリの情報を得る方法
1. 一般的なファイルシステムのように、ディレクトリエントリを作る方法

今回は実装の楽そうな後者のディレクトリエントリ方式で行います。

必要そうなのは以下のデータ

|列名 | 型 | 概要|
|---|---|---|
|parent_id|int|親ディレクトリのinode番号 (pkey)(foreign key)|
|child_id|int|子ファイル/子ディレクトリのinode番号 (foreign key)|
|file_type|int|ファイルタイプ|
|name|text|ファイル/ディレクトリ名 (pkey)|

あらゆるディレクトリは `.` と `..` のエントリを持ちます。(ルートの `..` は `.` です。)  
`.` と `..` は返さなくともよい事になっていますが、その場合は呼び出し側の責任で処理する事になります。

ファイルタイプはメタデータとディレクトリエントリで2重に持っていますが、ファイルタイプを変更する機能は無いのでよしとします。

## SQL
テーブル作成SQLは次のようになります。

```sqlite
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
FUSEでは、ルートディレクトリのinode番号は1です。ルートディレクトリは必ず存在する必要があります。

# Hello!
## 概要
第一段階として、rust-fuseに付属する、サンプルプログラムの `HelloFS` と同じ機能を実装します。
`HelloFS` は以下の機能があります。

1. ファイルシステムはリードオンリー
1. ルート直下に `hello.txt` というファイルがあり、 `"Hello World!\n"` という文字列が書き込まれている

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
今回作成した関数は以下になります。

```
pub trait DbModule {
    /// ファイルのメタデータを取得する。見つからない場合は0を返す
    fn get_inode(&self, inode: u32) -> Result<DBFileAttr, SqError>;
    /// ディレクトリのinode番号を指定して、ディレクトが持つディレクトリエントリを全て取得する
    fn get_dentry(&self, inode: u32) -> Result<Vec<DEntry>, SqError>;
    /// 親ディレクトリのinode番号と名前から、ファイルやサブディレクトリのinode番号とメタデータを得る
    /// inodeが存在しない場合、inode番号が0の空のinodeを返す
    fn lookup(&self, parent: u32, name: &str) -> Result<DBFileAttr, SqError>;
    /// inode番号とブロック数を指定して、1ブロック分のデータを読み込む
    /// ブロックデータが存在しない場合は、0(NULL)で埋められたブロックを返す
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
`ReplyEmpty, ReplyData, ReplyAttr` のように、関数に応じて `reply` の型が決まっています。

`reply.ok()` `reply.error(ENOSYS)` `reply.attr(...)` 等 `reply` の型に応じたメソッドが使えます。

## lookup
親ディレクトリのinode番号、当該ディレクトリ/ファイルの名前が与えられるので、ディレクトリエントリとメタデータを返します。  
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
ただし、この値をチェックするのは(知られているものでは)nfsしかありません。  
今回は常時 `0` に設定します。

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
引数のinode番号で指定されたファイルのメタデータを返します。
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
引数のinode番号で指定されたファイルをoffsetバイト目からsizeバイト分読み込みます。  
読み込んだデータは `reply.data(&data)` を実行して返します。

ファイルの読み込む位置を指定する方法は色々とありますが、fuseは `pread(2)` 相当の関数を一つ実装するだけで済むようにしてくれています。

EOFまたはエラーを返す場合を除いて、readはsizeで指定されたサイズのデータを返さないといけません。実データが足りなくて返せない場合は0(¥0)埋めします。  
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

引数の処理はそのうちclapを使うことになるでしょう。マウントオプションとかあるので。

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

```sqlite
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
試しに `cat ~/mount/hello.txt` を実行すると、以下のようなログが出力されます。 `env_logger` のおかげで各関数に対する呼び出しが記録されています。

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

## まとめ
Readonlyのファイルシステムが作成できました。  
次回はファイルの読み書きができるようにします。

# ReadWrite
## 概要
前回は、ファイルの読み込みができるファイルシステムを作成しました。
今回は、それに加えてファイルの書き込みができるようにします。

必要なのは以下の関数です。

```
fn write(&mut self, _req: &Request<'_>, _ino: u64, _fh: u64, _offset: i64, _data: &[u8], _flags: u32, reply: ReplyWrite) {
    ...
}

fn setattr(&mut self, _req: &Request<'_>, _ino: u64, _mode: Option<u32>, _uid: Option<u32>, _gid: Option<u32>, _size: Option<u64>, _atime: Option<Timespec>, _mtime: Option<Timespec>, _fh: Option<u64>, _crtime: Option<Timespec>, _chgtime: Option<Timespec>, _bkuptime: Option<Timespec>, _flags: Option<u32>, reply: ReplyAttr) {
    reply.error(ENOSYS);
}
```

なお、以下では実装する関数と同名のシステムコールと区別をつけるために、 システムコールは `write(2)` のような表記をします。

## DB関数
今回追加したDB側の関数は以下になります。

```
    /// inodeのメタデータを更新する。
    fn update_inode(&self, attr: DBFileAttr) -> Result<(), SqError>;
    /// 1ブロック分のデータを書き込む
    fn write_data(&self, inode:u32, block: u32, data: &[u8], size: u32) -> Result<(), SqError>;
```

## write
引数の `inode` で指定されたファイルに `data` で渡ってきたデータを書き込みます。

`write(2)` のようなシステムコールを使う場合はファイルオフセットを意識する必要がありますが、
fuseはカーネルがオフセットの管理をしてくれているので、 `pwrite(2)` 相当の関数を一つ実装するだけで済むようになっています。

マウントオプションに `direct_io` が設定されていない場合、エラーを返す場合を除いて、writeはsizeで指定された数字をreplyで返さないといけません。

引数の `fh` は `open` 時にファイルシステムが指定した値です。今回はまだopenを実装していないので、常に0になります。

また、 `open` 時のフラグに `O_APPEND` が設定されている場合は適切に処理しなければなりません。

### O_APPEND
ライトバックキャッシュが有効か無効かの場合で動作が異なります。
マウントオプションに `-o writeback` がある場合、ライトバックキャッシュが有効になっています。

ライトバックキャッシュが無効の時、ファイルシステムは `O_APPEND` を検知して、
全ての `write` の中で `offset` の値にかかわらずデータがファイル末尾に追記されるようにチェックします。

ライトバックキャッシュが有効の時、 `offset` はカーネルが適切に設定してくれます。 `O_APPEND` は無視してください。

実際には `O_APPEND` に対して適切に処理していないファイルシステムが多く、(今のところ)カーネルはどのような場合でも `offset` をきちんと設定してくれます。  
なので、現状は `O_APPEND` は無視し、 `open` 実装時に対応します。

### ここまでのコード
```
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

```
$ echo "append" >> ~/mount/hello.txt
$ cat ~/mount/hello.txt
Hello world!
append
```


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

特にファイルサイズは、追記などで書き込まれたデータの末尾が既存のファイルサイズより後になる場合は必ず更新する必要があります。  
また、書き込みのオフセットにファイルサイズより大きい値が指定された場合、ファイルに何も書かれていない穴ができます。
このエリアのデータが読まれた場合、ファイルシステムは0(NULLバイト)の列を返します。

マウントオプションで `-o noatime` が指定された場合、 `atime` の更新は行いません。

### タイムスタンプ
各関数とどのタイムスタンプを更新すべきかの対応表を示します。  
[Linuxプログラミングインターフェース](https://www.oreilly.co.jp/books/9784873115856/)
のシステムコールとタイムスタンプの対応表を参考に、FUSEの関数にマップしました。

|関数名|a|m|c|親ディレクトリのa|m|c|備考|
|---|---|---|---|---|---|---|---|
|setattr|||o|||||
|setattr||o|o||||ファイルサイズが変わる場合|
|link|||o||o|o||
|mkdir|o|o|o||o|o||
|mknod|o|o|o||o|o||
|create|o|o|o||o|o|新規作成時|
|open, create|o|o|o||||O_TRUNCの場合|
|read|o|||||||
|readdir|o|||||||
|setxattr|||o|||||
|removexattr|||o|||||
|rename|||o||o|o|移動前/移動後の両方の親ディレクトリを変更|
|rmdir|||||o|o||
|symlink|o|o|o||o|o|リンク自体のタイムスタンプで、リンク先は変更しない|

## setattr
`write` は実装しましたが、このままでは追記しかできません。  
ファイルを丸ごと更新するために、ファイルサイズを0にする(truncateに相当) 処理を実装します。

rust-fuseでは、 `setattr` を実装する事でファイルサイズの変更が可能になります。

`setattr` は引数に `Option` で値が指定されるので、中身がある場合はその値で更新していきます。  
`reply` に入れる値は、更新後のメタデータです。

なお、 `ctime` は 現在のrust-fuseがプロトコルのバージョンの問題で未対応なので、引数には入っていません。

`truncate(2)` でファイルサイズが変わる場合、元のファイルサイズより小さい値が指定された場合、差分のデータがきちんと破棄されるように、  
元のファイルサイズより大きい値が指定された場合、間のデータが0(\0)で埋められるように気をつけてください。

```
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
    // 引数で上書き
    if let Some(n) = mode {attr.perm = n as u16};
    if let Some(n) = uid {attr.uid = n};
    if let Some(n) = gid {attr.gid = n};
    if let Some(n) = size {attr.size = n as u32};
    if let Some(n) = atime {attr.atime = datetime_from_timespec(&n)};
    if let Some(n) = mtime {attr.mtime = datetime_from_timespec(&n)};
    attr.ctime = SystemTime::now();
    if let Some(n) = crtime {attr.crtime = datetime_from_timespec(&n)};
    if let Some(n) = flags {attr.flags = n};
    // 更新
    match self.db.update_inode(attr) {
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
これでファイルの書き込みができるようになりました。  
次回は、ファイルの作成と削除を実装します。

# ファイルの作成と削除
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
    ...(追加)
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
`lib.rs` や `fuse_lowlevel.h` によると、
「lookup count が0でない内は、unlink, rmdir, rename(で上書き)されて参照カウントが0になってもinodeを削除しないでね」という事です。  
lookup countは最初は0で、ReplyEntryとReplyCreateがある全ての関数が呼ばれるたびに、1ずつ増やしていきます。  
具体的には、 `lookup`, `mknod`, `mkdir`, `symlink`, `link`, `create` です。

`forget` はlookup countを減らす関数です。 `forget` でlookup countが0になるまでは、削除を遅らせます。  
カーネルは、まだファイルをOpenしているプロセスがあると、 `forget` をファイルが閉じられるまで遅延させます。  
これにより、「別の誰かがファイルを削除したが、削除前からファイルを開いていた場合は読み込み続ける事ができる」というアレが実現できます。

lookup countを実装するために、ファイルシステムの構造体に次の変数を追加します。

```
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

```
/// inodeを追加する
fn add_inode(&mut self, parent: u32, name: &str, attr: &DBFileAttr) -> Result<u32, SqError>;
/// inodeをチェックし、参照カウントが0なら削除する
fn delete_inode_if_noref(&mut self, inode: u32) -> Result<(), SqError>;
/// 親ディレクトリのinode番号、ファイル/ディレクトリ名で指定されたディレクトリエントリを削除し、
/// 該当のinodeの参照カウントを1減らす
/// inode番号を返す
fn delete_dentry(&mut self, parent: u32, name: &str) -> Result<u32, SqError>;
/// 参照カウントが0である全てのinodeを削除する
fn delete_all_noref_inode(&mut self) -> Result<(), SqError>;
```

## lookup
`lookup` 関数を更新します。  
関数が実行されるたびに、lookupで情報を持ってくる対象のファイル/ディレクトリのlookup countに1を足すようにします。

コードは以下のようになります。

```
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

    // lookup countに1を足す。HashMapにkeyが無い場合は追加する
    let mut lc_list = self.lookup_count.lock().unwrap();
    let lc = lc_list.entry(child).or_insert(0);
    *lc += 1;
}
```

## create
ファイルを作成します。

`creat(2)`または `O_CREAT` を指定した `open(2)` 実行時に呼ばれます。

指定されたファイルが存在しない場合、引数の `mode` で指定されたモードでファイルを作成し、ファイルを開きます。  
作成したユーザ、グループは、引数の `req` から `req.uid()` `req.gid()` で取得できます。
ファイルが既に存在する場合、openと同じ動作を行います。  
`open` と同じ動作のため、open時のフラグが `flags` で渡されます。その他処理は `open` と同じです。

`create` が実装されていない場合、カーネルは `mknod` と `open` を実行します。

なお、`create` が実装されている場合、libfuseは通常ファイルの `mknod` が実行されると `create` を呼び出しますが、
rust-fuseは呼び出してくれないです。

実装したコードは以下のようになります。

```
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
            ino: 0,
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
親ディレクトリのinode番号 `parent`, ファイル/ディレクトリ名 `name` が引数で指定されるので、
ファイルまたはディレクトリを削除します。

削除対象はディレクトリエントリと、該当のinodeのメタデータです。
inodeはハードリンクされている可能性があるので、参照カウント( `nlink` ) を1減らし、0になった場合に削除します。  
また、 lookup count をチェックし、0になっていない場合は削除を行いません。

```
fn unlink(&mut self, _req: &Request<'_>, parent: u64, name: &OsStr, reply: ReplyEmpty) {
    // ディレクトリエントリを削除しつつ、対象のinode番号を得る
    let ino = match self.db.delete_dentry(parent as u32, name.to_str().unwrap()) {
        Ok(n) => n,
        Err(err) => {reply.error(ENOENT); debug!("{}", err); return;}
    };
    // lookup countのチェック
    let lc_list = self.lookup_count.lock().unwrap();
    if !lc_list.contains_key(&ino) {
        リンクカウントが0の場合削除する
        match self.db.delete_inode_if_noref(ino) {
            Ok(n) => n,
            Err(err) => {reply.error(ENOENT); debug!("{}", err); return;}
        };
    }
    reply.ok();
}
```

## forget
lookup countを忘れます。

引数の `ino` で対象のinode番号、 `nlookup` で減らす数が指定されます。  

```
fn forget(&mut self, _req: &Request<'_>, ino: u64, nlookup: u64) {
    let ino = ino as u32;
    // lookup countのチェック
    let mut lc_list = self.lookup_count.lock().unwrap();
    let lc = lc_list.entry(ino).or_insert(0);
    *lc -= nlookup as u32;
    if *lc <= 0 {
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
ファイルシステムの終了時に呼ばれる関数です。

ファイルシステムのアンマウント時には、全ての lookup count が0になる事が期待されます。  
一方、 `forget` が呼ばれる事は保証されていないので、ファイルシステムが自分でチェックする必要があります。

```
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
ファイルシステムのマウント時に最初に呼ばれる関数です。

何らかの事情で `destroy` が呼ばれずにファイルシステムが終了した場合、参照カウントが0のままのinodeが残り続ける事になるので、
チェックして削除します。

```
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

```
$ touch ~/mount/touch.txt
$ echo "created" > ~/mount/test.txt
$ ls ~/mount
hello.txt  test.txt  touch.txt
$ cat ~/mount/test.txt
created
```

```
[2019-10-30T11:21:59Z DEBUG fuse::request] INIT(2)   kernel: ABI 7.31, flags 0x3fffffb, max readahead 131072
[2019-10-30T11:21:59Z DEBUG fuse::request] INIT(2) response: ABI 7.8, flags 0x1, max readahead 131072, max write 16777216
[2019-10-30T11:22:14Z DEBUG fuse::request] LOOKUP(4) parent 0x0000000000000001, name "touch.txt"
[2019-10-30T11:22:14Z DEBUG fuse::request] CREATE(6) parent 0x0000000000000001, name "touch.txt", mode 0o100664, flags 0x8841
[2019-10-30T11:22:14Z DEBUG fuse::request] FLUSH(8) ino 0x0000000000000003, fh 0, lock owner 16194556409419452441
[2019-10-30T11:22:14Z DEBUG fuse::request] SETATTR(10) ino 0x0000000000000003, valid 0x1b0
[2019-10-30T11:22:14Z DEBUG fuse::request] RELEASE(12) ino 0x0000000000000003, fh 0, flags 0x8801, release flags 0x0, lock owner 0
[2019-10-30T11:22:28Z DEBUG fuse::request] LOOKUP(14) parent 0x0000000000000001, name "test.txt"
[2019-10-30T11:22:28Z DEBUG fuse::request] CREATE(16) parent 0x0000000000000001, name "test.txt", mode 0o100664, flags 0x8241
[2019-10-30T11:22:29Z DEBUG fuse::request] GETXATTR(18) ino 0x0000000000000004, name "security.capability", size 0
[2019-10-30T11:22:29Z DEBUG fuse::request] WRITE(20) ino 0x0000000000000004, fh 0, offset 0, size 8, flags 0x0
[2019-10-30T11:22:29Z DEBUG fuse::request] RELEASE(22) ino 0x0000000000000004, fh 0, flags 0x8001, release flags 0x0, lock owner 0
```

### ファイル削除

```
$ rm ~/mount/test.txt
$ ls ~/mount/test.txt
ls: cannot access '/home/jiro/mount/test.txt': No such file or directory
```


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
今まではルートディレクトリのみでファイル操作を行っていましたが、
今回はディレクトリの作成/削除を実装して、サブディレクトリでいろいろできるようにします。

## 実装すべき関数
```
fn mkdir(&mut self, _req: &Request<'_>, _parent: u64, _name: &OsStr, _mode: u32, reply: ReplyEntry) {
    ...
}
fn rmdir(&mut self, _req: &Request<'_>, _parent: u64, _name: &OsStr, reply: ReplyEmpty) {
    ...
}
```

## 追加したDB関数

```

```

## mkdir
引数で親ディレクトリのinode番号、ディレクトリ名、モードが指定されるので、ディレクトリを作成します。
成功した場合、作成したディレクトリのメタデータを返します。

動作、エラーなどは `mkdir(2)` に従い、

## rmdir
引数で親ディレクトリのinode番号とディレクトリ名が指定されるので、ディレクトリを削除します。

当然ながらディレクトリ内になにかある場合は削除できません。  
ファイルシステム側でチェックを行い、ディレクトリが空ではない( `.` と `..` 以外のエントリがある) 場合はエラーを返します。  
`rmdir(2)` のmanによると、 `ENOTEMPTY` または `EEXIST` を返します。Linuxファイルシステムでは `ENOTEMPTY` がメジャーのようです。

`unlink` と同様に、 `lookup count` が0でない場合、0になるタイミングまでinodeの削除を遅延します。  
`forget` 等の内部ではファイルとディレクトリの区別をしていないので、現状の実装でOKです。
