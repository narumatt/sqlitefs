# plan

## ロードマップ
1. Hello
1. ReadWrite
1. create / delete file
1. create / delete directory
1. rename
1. link, symlink
1. additional functions and errors
1. permissions
1. lock operation
1. extended attributes

## 未実装リスト

- flush
- fsync
- mknod
- fsyncdir
- statfs
- setxattr
- getxattr
- listxattr
- removexattr
- access
- getlk
- setlk
- bmap


- sgid対応
- O_APPEND
- direct_io
- マウントオプションの処理

## 参考リンク

[rust-fuse](https://github.com/zargony/rust-fuse) : Rust版Fuseプロジェクト

[libfuse](https://github.com/libfuse/libfuse) : C版のFuseインターフェースライブラリ

[osxfuse](https://github.com/osxfuse/fuse) : MacOS向けのFuseインターフェースライブラリ

[FUSEプロトコルの説明](https://john-millikin.com/the-fuse-protocol) : カーネルモジュール <-> Fuseライブラリ間のプロトコル

[VFSの説明](https://ja.osdn.net/projects/linuxjf/wiki/vfs.txt)

[lowlevel関数の説明(libfuseのヘッダ)](https://github.com/libfuse/libfuse/blob/master/include/fuse_lowlevel.h)

[ファイルオープン時のもろもろの説明(libfuseのヘッダ)](https://github.com/libfuse/libfuse/blob/master/include/fuse_common.h)

[Linuxプログラミングインターフェース(書籍)](https://www.oreilly.co.jp/books/9784873115856/) : システムコールの満たすべき要件

[libfuseのメーリングリストのアーカイブ](https://sourceforge.net/p/fuse/mailman/fuse-devel/)

[gcsf(rust-fuseの実装例)](https://github.com/harababurel/gcsf)

## データベース構造
テーブルはメタデータ(MDT)とディレクトリエントリ(DET)とブロックデータ(BDT)と拡張属性データ(XATTRT)の4つに分ける。

### MDT
メタデータは一般的なファイルシステムのメタデータと同様で、fuseが必要なデータを持つ。

idをinteger primary keyにする。これをinode番号とする。

必要そうな情報達

```
pub struct FileAttr {
    /// Inode number
    pub ino: u64,
    /// Size in bytes
    pub size: u64,
    /// Size in blocks Sparse File に対応する場合、実際に使用しているブロック数を返す
    pub blocks: u64,
    /// Time of last access read(2)実行時に更新される
    pub atime: Timespec,
    /// Time of last modification write(2)またはtruncate(2)実行時に更新される
    pub mtime: Timespec,
    /// Time of last change メタデータ変更時に更新される。 write(2)またはtruncate(2)でファイル内容が変わるときも更新される
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
    /// Rdev
    pub rdev: u32,
    /// Flags (macOS only, see chflags(2))
    pub flags: u32,
}
```

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

mknod時のkindはmodeから得る。 `libc::S_IFREG` 等を使うとよい。  
なぜか `S_ISREG` は無い…

それ以外(create, mkdir)の場合ファイル種類は自明である。

### BDT
BDTのblobにデータを格納する。
BDTはファイルのinode, 何番目のブロックか、の列を持つ

|列名 | 型 | 概要|
|---|---|---|
|file_id|int|ファイルのinode番号 (pkey)(foreign key)|
|block_num|int|データのブロック番号(pkey)|
|data|blob|データ(4kByte単位とする)|

`foreign key (file_id) references metadata(id) on delete cascade`
を指定する事で、ファイルのメタデータが消えたらデータも削除されるようにする。

`primary key (file_id, block_num)`
を指定する。

### DET
ディレクトリ構造を表現する方法は、以下の2つの候補がある

1. 分散ファイルシステムでよくある、フルパスを各ファイルが持っていて、文字列操作で各ディレクトリの情報を得る方法
1. 一般的なファイルシステムのように、ディレクトリエントリを作る方法

今回は実装の楽そうな後者のディレクトリエントリ方式で行う。
ext2のディレクトリエントリが分かりやすいので、似たようなのを作る。

必要そうなのは以下のデータ

|列名 | 型 | 概要|
|---|---|---|
|parent_id|int|親ディレクトリのinode (pkey)(foreign key)|
|child_id|int|子ファイル/子ディレクトリのinode (foreign key)|
|file_type|int|ファイルタイプ|
|name|text|ファイル/ディレクトリ名 (pkey)|

あらゆるディレクトリは `.` と `..` のエントリを持つ

### XATTRT
拡張ファイル属性を格納する。

|列名 | 型 | 概要|
|---|---|---|
|file_id|int|ファイルのinode番号 (pkey)(foreign key)|
|name|text|属性名(pkey)|
|value|blob|値|

## ルートディレクトリ
fuseではルートディレクトリのinodeは1である。

また、一般的にルートディレクトリの `..` はルートディレクトリ自身を指す

## fuseの関数

用意されている関数は、cの `fuse_lowlevel` 内の関数と同じである。
トレイトの関数は以下の通り。詳しい内容は `lib.rs` のコメントを参照すること。  
なお、以下の関数全てを実装する必要はない。実装しない関数は失敗するだけである。

以降の説明で、「カーネル」とは、カーネルコールに応じてfuseの関数を呼び出す存在であり、
「ファイルシステム」とは、あなたの書いたコードであることに注意する。

システムコールやlibcの同名の関数と区別するため、システムコールは `open(2)` 、標準Cライブラリ関数は `opendir(3)` のように表記する。

```
pub trait Filesystem {
    fn init(&mut self, _req: &Request<'_>) -> Result<(), c_int> {
        Ok(())
    }
    fn destroy(&mut self, _req: &Request<'_>) {}
    fn lookup(&mut self, _req: &Request<'_>, _parent: u64, _name: &OsStr, reply: ReplyEntry) {
        reply.error(ENOSYS);
    }
    fn forget(&mut self, _req: &Request<'_>, _ino: u64, _nlookup: u64) {}
    fn getattr(&mut self, _req: &Request<'_>, _ino: u64, reply: ReplyAttr) {
        reply.error(ENOSYS);
    }
    fn setattr(&mut self, _req: &Request<'_>, _ino: u64, _mode: Option<u32>, _uid: Option<u32>, _gid: Option<u32>, _size: Option<u64>, _atime: Option<Timespec>, _mtime: Option<Timespec>, _fh: Option<u64>, _crtime: Option<Timespec>, _chgtime: Option<Timespec>, _bkuptime: Option<Timespec>, _flags: Option<u32>, reply: ReplyAttr) {
        reply.error(ENOSYS);
    }
    fn readlink(&mut self, _req: &Request<'_>, _ino: u64, reply: ReplyData) {
        reply.error(ENOSYS);
    }
    fn mknod(&mut self, _req: &Request<'_>, _parent: u64, _name: &OsStr, _mode: u32, _rdev: u32, reply: ReplyEntry) {
        reply.error(ENOSYS);
    }
    fn mkdir(&mut self, _req: &Request<'_>, _parent: u64, _name: &OsStr, _mode: u32, reply: ReplyEntry) {
        reply.error(ENOSYS);
    }
    fn unlink(&mut self, _req: &Request<'_>, _parent: u64, _name: &OsStr, reply: ReplyEmpty) {
        reply.error(ENOSYS);
    }
    fn rmdir(&mut self, _req: &Request<'_>, _parent: u64, _name: &OsStr, reply: ReplyEmpty) {
        reply.error(ENOSYS);
    }
    fn symlink(&mut self, _req: &Request<'_>, _parent: u64, _name: &OsStr, _link: &Path, reply: ReplyEntry) {
        reply.error(ENOSYS);
    }
    fn rename(&mut self, _req: &Request<'_>, _parent: u64, _name: &OsStr, _newparent: u64, _newname: &OsStr, reply: ReplyEmpty) {
        reply.error(ENOSYS);
    }
    fn link(&mut self, _req: &Request<'_>, _ino: u64, _newparent: u64, _newname: &OsStr, reply: ReplyEntry) {
        reply.error(ENOSYS);
    }
    fn open(&mut self, _req: &Request<'_>, _ino: u64, _flags: u32, reply: ReplyOpen) {
        reply.opened(0, 0);
    }
    fn read(&mut self, _req: &Request<'_>, _ino: u64, _fh: u64, _offset: i64, _size: u32, reply: ReplyData) {
        reply.error(ENOSYS);
    }
    fn write(&mut self, _req: &Request<'_>, _ino: u64, _fh: u64, _offset: i64, _data: &[u8], _flags: u32, reply: ReplyWrite) {
        reply.error(ENOSYS);
    }
    fn flush(&mut self, _req: &Request<'_>, _ino: u64, _fh: u64, _lock_owner: u64, reply: ReplyEmpty) {
        reply.error(ENOSYS);
    }
    fn release(&mut self, _req: &Request<'_>, _ino: u64, _fh: u64, _flags: u32, _lock_owner: u64, _flush: bool, reply: ReplyEmpty) {
        reply.ok();
    }
    fn fsync(&mut self, _req: &Request<'_>, _ino: u64, _fh: u64, _datasync: bool, reply: ReplyEmpty) {
        reply.error(ENOSYS);
    }
    fn opendir(&mut self, _req: &Request<'_>, _ino: u64, _flags: u32, reply: ReplyOpen) {
        reply.opened(0, 0);
    }
    fn readdir(&mut self, _req: &Request<'_>, _ino: u64, _fh: u64, _offset: i64, reply: ReplyDirectory) {
        reply.error(ENOSYS);
    }
    fn releasedir(&mut self, _req: &Request<'_>, _ino: u64, _fh: u64, _flags: u32, reply: ReplyEmpty) {
        reply.ok();
    }
    fn fsyncdir (&mut self, _req: &Request<'_>, _ino: u64, _fh: u64, _datasync: bool, reply: ReplyEmpty) {
        reply.error(ENOSYS);
    }
    fn statfs(&mut self, _req: &Request<'_>, _ino: u64, reply: ReplyStatfs) {
        reply.statfs(0, 0, 0, 0, 0, 512, 255, 0);
    }
    fn setxattr(&mut self, _req: &Request<'_>, _ino: u64, _name: &OsStr, _value: &[u8], _flags: u32, _position: u32, reply: ReplyEmpty) {
        reply.error(ENOSYS);
    }
    fn getxattr(&mut self, _req: &Request<'_>, _ino: u64, _name: &OsStr, _size: u32, reply: ReplyXattr) {
        reply.error(ENOSYS);
    }
    fn listxattr(&mut self, _req: &Request<'_>, _ino: u64, _size: u32, reply: ReplyXattr) {
        reply.error(ENOSYS);
    }
    fn removexattr(&mut self, _req: &Request<'_>, _ino: u64, _name: &OsStr, reply: ReplyEmpty) {
        reply.error(ENOSYS);
    }
    fn access(&mut self, _req: &Request<'_>, _ino: u64, _mask: u32, reply: ReplyEmpty) {
        reply.error(ENOSYS);
    }
    fn create(&mut self, _req: &Request<'_>, _parent: u64, _name: &OsStr, _mode: u32, _flags: u32, reply: ReplyCreate) {
        reply.error(ENOSYS);
    }
    fn getlk(&mut self, _req: &Request<'_>, _ino: u64, _fh: u64, _lock_owner: u64, _start: u64, _end: u64, _typ: u32, _pid: u32, reply: ReplyLock) {
        reply.error(ENOSYS);
    }
    fn setlk(&mut self, _req: &Request<'_>, _ino: u64, _fh: u64, _lock_owner: u64, _start: u64, _end: u64, _typ: u32, _pid: u32, _sleep: bool, reply: ReplyEmpty) {
        reply.error(ENOSYS);
    }
    fn bmap(&mut self, _req: &Request<'_>, _ino: u64, _blocksize: u32, _idx: u64, reply: ReplyBmap) {
        reply.error(ENOSYS);
    }
    #[cfg(target_os = "macos")]
    fn setvolname(&mut self, _req: &Request<'_>, _name: &OsStr, reply: ReplyEmpty) {
        reply.error(ENOSYS);
    }
    #[cfg(target_os = "macos")]
    fn exchange(&mut self, _req: &Request<'_>, _parent: u64, _name: &OsStr, _newparent: u64, _newname: &OsStr, _options: u64, reply: ReplyEmpty) {
        reply.error(ENOSYS);
    }
    #[cfg(target_os = "macos")]
    fn getxtimes(&mut self, _req: &Request<'_>, _ino: u64, reply: ReplyXTimes) {
        reply.error(ENOSYS);
    }
}
```

### 戻り値
各関数に戻り値は存在せず、 `reply` 引数を操作して、呼び出し元に値を受け渡す。

`reply.ok()` `reply.error(ENOSYS)` `reply.attr(...)` 等を使う。

### init
マウント後最初に呼ばれる。初期化が必要な場合、ここで行う。

今回はデータベースの接続と、必要なら初期化を行う。

### destroy
アンマウント時に呼ばれる。データベースから切断する。

### lookup
親ディレクトリのinode、当該ディレクトリ/ファイルの名前が与えられるので、ディレクトリエントリを返す。
この時、inodeの lookup count を1増やさなければならない(forgetで0に戻す)

lookup count については、 `lib.rs` によると、
「lookup count がある内は参照カウントが0になってもinodeを削除しないでね」という話  
以降ReplyEntry と ReplyCreateがある全ての関数が呼ばれるたびに、1ずつ増やしていく。

[forgetについての議論](http://fuse.996288.n3.nabble.com/forget-inodes-td9599.html)

必要なデータは以下の通り

```
    //正常
    reply.entry(&TTL, &ATTR, &GENERATION);
    エラー
    reply.error(ENOENT);
```

- TTL

`std::time::Duration` で期間を指定する。  
TTLの間はカーネルは再度問い合わせに来ない。

- ATTR

対象の情報。 `fuse::FileAttr` を返す

- generation

inodeの世代情報。削除されたinodeに別のファイルを割り当てた場合、
前のファイルと違うファイルである事を示すために、generationに別の値を割り当てる。  
ただし、この値をチェックするのは(知られているものでは)nfsしかない。  
今回はinodeの使い回しが無いので、常時 `0` に設定する

[libfuseの説明](https://libfuse.github.io/doxygen/structfuse__entry__param.html#a4c673ec62c76f7d63d326407beb1b463)

- ENOENT

対象のディレクトリエントリが存在しない場合

### forget
対象のinodeの参照カウントを nlookup だけ減らす。  
`The filesystem may ignore forget calls, if the inodes don't need to have a limited lifetime.`  
だそうです。ファイルが削除できるファイルシステムの場合は注意しないといけない。

アンマウント時にはforgetが呼ばれる事が保証されないが、全てのinodeのnlookupが0になる事が期待される。

### getattr
ファイルの属性を返す。
内容については `lookup` で返す `ATTR` と同じ。

### setattr
ファイルの属性を設定する。
引数にOptionで各属性がぞろぞろとやってくるので、設定していく。

```
_ino: u64,
_mode: Option<u32>,
_uid: Option<u32>,
_gid: Option<u32>,
_size: Option<u64>,
_atime: Option<Timespec>,
_mtime: Option<Timespec>,
_fh: Option<u64>,
_crtime: Option<Timespec>,
_chgtime: Option<Timespec>,
_bkuptime: Option<Timespec>
 _flags: Option<u32>`


```

返す値はgetattrと同じで、設定後の属性一覧を返す。

### readlink
シンボリックリンクを読み込む。
対象のフルパスの文字列をReplyDataに入れる。

### mknod
親ディレクトリのinode, ファイル名, モード, デバイス番号が指定されるので、ファイルまたはスペシャルファイルを作成する。
`create` が定義されている場合、通常ファイルについてはそちらが呼ばれる、と `libfuse` には書かれているが、rust-fuseはそういう動作をしない  
大抵の仮想ファイルシステムではスペシャルファイルはエラーでよいと思われる。mknodを実装していないシステムも多い。  
作成対象がどのファイルかは `_mode` で調べる。 例えば通常ファイルかどうか調べる場合は `libc::S_IFREG` を使うとよい。

### mkdir
親ディレクトリのinode, ディレクトリ名, モードが指定されるので、ディレクトリを作る。
成功した場合、attrを返す

動作、エラーなどは `mkdir(2)` に従う

### unlink
親ディレクトリのinode, ディレクトリ名が指定されるので、ファイルを削除する。

`lookup count` が0でない場合、0になるタイミングまで削除を遅延する。

### rmdir
親ディレクトリのinode, ディレクトリ名が指定されるので、ディレクトリを削除する。

当然ながらディレクトリ内になにかある場合は削除できない。

`lookup count` が0でない場合、0になるタイミングまで削除を遅延する。

### symlink
親ディレクトリのinode, シンボリックリンク名, シンボリックリンク先が与えられるので、シンボリックリンクを作成する。

### rename
親ディレクトリのinode, 名前, 変更後の親ディレクトリのinode, 変更後の名前が指定されるので、ファイルまたはディレクトリ名前を変更する。

cの `fuse_lowlevel` の説明によると、変更先が存在する場合は自動で上書きしなければならない。  
変更先の `lookup count` が0でない場合は、削除処理と同様0になるまでinodeの削除を遅延する。

`ENOSYS` を返した場合、後続のbmap等の処理はファイルシステムに渡される前に失敗する。

cだと 上書き禁止を指定したりできる `flag` が指定されるが、このライブラリには無いようである。

### link
対象のinode, 親ディレクトリのinode, 名前が与えられるので、ハードリンクを作成する。

### open
inodeで指定されたファイルを開く。

#### 引数のフラグ
フラグは `_flags` 引数で渡される。
ただし、 `O_CREAT, O_EXCL, O_NOCTTY` の3つはカーネルで省かれるので、ファイルシステムは検知できない。

ファイルシステムは、アクセスモードのフラグを使ってアクセス権チェックを行わないといけない。  
ただし、マウントオプションで `-o default_permissions` が指定されている場合はカーネルがチェックしてくれるので、
このオプションがある場合は何もしなくてよい。

ライトバックキャッシュが有効の時、カーネルは `O_WRONLY` でもreadしてくる事がある。  
マウントオプションで `-o writeback` が有効の場合は読めるようにしておく。  
[libfuseのサンプルの修正例](https://github.com/libfuse/libfuse/commit/b3109e71faf2713402f70d226617352815f6c72e)
を見るとよい。

ライトバックキャッシュが無効の時、ファイルシステムは `O_APPEND` フラグを適切に扱う必要がある。  
つまり、 `O_APPEND` を検知して、全ての `write` の中で `offset` の値にかかわらずデータがファイル末尾に追記されるようにチェックしなければならない。

ライトバックキャッシュが有効の時、 `O_APPEND` はカーネルが扱う。 `offset` はカーネルが適切に設定してくれる。
ファイルシステムは無視するか、エラーを返さないといけない。  
先述のlibfuseのサンプルの修正例を参考にすること。

ここに書かれていないフラグも、 `open(2)` の定義通りに動作しなければならない。  
[このページ](https://bugs.freebsd.org/bugzilla/show_bug.cgi?id=236340) によると、Linux4.9.0ではフラグの伝達は以下のような挙動になる

|flag|create|open|
|---|---|---|
|O_CREAT|yes|no|
|O_EXCL|yes|no|
|O_NOCTTY|no|no|
|O_TRUNC|yes|yes|
|O_APPEND|yes|yes|
|O_NONBLOCK|yes|yes|
|O_SYNC|yes|yes|
|O_ASYNC|yes|yes|
|O_LARGEFILE||yes, even if I don't ask for it!|
|O_DIRECTORY|N/A|N/A.  open is translated to OPENDIR|
|O_NOFOLLOW|yes|yes|
|O_CLOEXEC|no|no|
|O_DIRECT|yes|yes|
|O_NOATIME|yes|yes|
|O_PATH|N/A|N/A doesn't actually open anything|
|O_DSYNC|yes|yes|
|O_TMPFILE|N/A|N/A includes O_DIRECTORY|
|O_EXEC||Not implemented on Linux|



#### ファイルハンドル
ファイルシステムは `fh: u64` を戻り値に含めることができる( `reply.opened()` の1番目の引数)。

fhには、ポインター、インデックス、その他好きな値をファイルシステム側で定める事ができる。
この値は `read` `write` 等で引数として渡されるので、ファイルシステム自身で状態を持たずに済む。  
cでいうと、 `fi->fh` である。  
もちろんこの機能を使わなくともよい。

#### 戻り値のフラグ
ファイルシステムはフラグを戻り値に含めることができる( `reply.opened()` の2番目の引数)。  
`fuse-abi` の `FOPEN_DIRECT_IO` `FOPEN_KEEP_CACHE` `FOPEN_NONSEEKABLE` `FOPEN_PURGE_ATTR` `FOPEN_PURGE_UBC`
が相当する。(それぞれビットマスク)

通常はあまり使わないと思われるので、詳しく知りたい場合は [libfuseのfuse_common.h](https://github.com/libfuse/libfuse/blob/master/include/fuse_common.h)
を参照すること。

### read 
inodeで指定されたファイルをoffsetからsize分読み込む

ファイルの読み込む位置を指定する方法は色々とあるが、fuseは `pread(2)` 相当の関数を一つ実装するだけで済むようにしてくれている。

EOFまたはエラーを返す場合を除いて、readはsizeで指定されたサイズのデータを返さないといけない。実データが足りない場合は0埋めされる。  
例えば、長さ200byteのデータに対して、4096byteの要求が来ることがあるが、200byte返すと3896byte分を0埋めしたとみなされる。  
例外として、`direct_io` フラグを指定した場合、カーネルは `read(2)` システムコールの戻り値として、
ファイルシステムの戻り値を直接使うので、sizeより小さいデータを返してもよい。

引数の `fh` は `open` 時にファイルシステムが指定した値である。

### write
inodeで指定されたファイルにデータを書き込む

ファイルの書き込む位置を指定する方法は色々とあるが、fuseは `pwrite(2)` 相当の関数を一つ実装するだけで済むようにしてくれている。

`direct_io` が設定されていない場合、エラーを返す場合を除いて、writeはsizeで指定された数字を返さないといけない。

引数の `fh` は `open` 時にファイルシステムが指定した値である。

`open` の項で述べたように、 `O_APPEND` が設定されている場合は適切に処理しなければならない。

### flush
`close(2)` システムコールの度に呼ばれる。

`release` は値を返さないので、 `close(2)` に対してエラーを返したい場合はここで行う。

`dup, dup2, fork` によりプロセスが複製される事で、一つの `open` に対して、複数の `flush` が呼ばれる場合がある。  
どれが最後の `flush` なのか識別するのは不可能なので、後で(または `flush` 処理中に) 別の `flush` が呼ばれてもいいように対応しなければならない。  

例えば、sshfsでは、スレッド単位でロックをかけて書き込み処理の後始末を行っている。

`flush` という名前が付いてはいるが、 `fsync` のようにデータをディスクに書き込む事を義務付けられてはいない。  
`close(2)` 時にデータが書き込まれているかどうかは使用者側の責任である。

`setlk` `getlk` のようなファイルロック機構をファイルシステムが実装している場合、引数の `_lock_owner` が持つロックを全て開放すること。

### release
ファイルを閉じる。  
ファイルに対する参照が一つもなくなった場合に呼ばれる。一つの `open` に対して、一つの `release` が呼ばれる。

ファイルシステムはエラーを返してもよいが、呼び出し元の `close(2)` や `munmap(2)` には値が渡らないので、無意味である。

引数の `_fh` は `open` 時にファイルシステムが指定した値であり、 `_flags` は `open` 時の引数と同一の値である。

### fsync
ファイルを永続領域に書き込む。

fsyncが呼ばれるまでは、書き込まれたデータやメタデータはキャッシュしていてよい。  
つまり、なんらかの事情(kill, マシンの電源断)でファイルシステムのデーモンが即座に終了したとしても、データの保証はしなくてよい。

一方、fsyncに対して `reply.ok()` を返した時点で、データがディスクやネットワークの先などどこかの領域に保存されている事を保証しなければならない。

引数 `_datasync` が `true` である場合、メタデータは書き込まなくてもよい。

### opendir
ディレクトリを開く。

ファイル同様、 `fh: u64` を戻り値に含めることができる( `reply.opened()` の1番目の引数)。
`fh` に何も入れないことも可能だが、 `opendir` から `relasedir` までの間に
ディレクトリエントリが追加または削除された場合でも `readdir` の整合性を保つために、何らかの情報を入れておく事が推奨される。  
open中に追加または削除されたエントリは返しても返さなくても良いが、追加または削除されていないエントリは必ず返さないといけないので、
ディレクトリエントリをコピーして、先頭のポインタを格納する、等をするとよい。

### readdir
指定されたinodeのディレクトリのディレクトリエントリを返す。  
バッファが渡されるので、一杯になるまでディレクトリエントリを入れて返す。

引数の `_fh` は `opendir` でファイルシステムが渡した値である。

cでは `fuse_add_direntry()` を使用してバッファを埋めるが、rustでは渡された `reply: ReplyDirectory` を使用する。

```
result = reply.add(target_inode, offset, FileType.RegularFile, filename);
```

バッファが一杯の時、 `ReplyDirectory.add()` は `true` を返す。

`offset` はファイルシステムが任意に決めたオフセットである。  
何らかの意味を持つ値でなくともよいが、ファイルシステムは `offset` が与えられたとき、対応する一意のディレクトリエントリを求められなければならない。  
カーネルが`readdir`の 引数として `_offset` に0でない値を指定してきた場合、
該当の `offset` を持つディレクトリエントリの次のディレクトリエントリから返さなければならない。  
つまり、 `offset` は「次のディレクトリエントリのoffset」を意味する。  
`0` は「最初のディレクトリエントリ」を指すので、 `offset` に0を入れてはならない。

`.` と `..` は返さなくともよいが、返さなかった場合の処理は呼び出し側のプログラムに依存する。

`reply.add()` でデータを追加していき、最終的に `reply.ok()` を実行すると、データが返せる。

### releasedir
`opendir` で確保したリソースを解放する。  
引数の `_fh` は `opendir` でファイルシステムが渡した値である。

一度の `opendir` に対して、一度だけ `releasedir` が呼び出される。

### fsyncdir
ディレクトリのデータ(ディレクトリエントリ)をディスクに書き込む。

引数の `_datasync` が `true` の時、メタデータを更新しない。

サンプルを見るとディレクトリに `fsync(2)` した時の挙動でいい様な感じがする。  
`sshfs` や `s3fs` で実装されていないので、実装の優先度は低い。

### statfs
`statfs(2)` で使うファイルシステムの情報を返す。

```
blocks: u64; // frsize単位での総ブロック数 (ex: 1024)
bfree: u64; // 空きブロック数
bavail: u64; // スーパーユーザ用に予約されている領域を除いた空きブロック数
files: u64; // 総inode数
ffree: u64; // 空きinode数
bsize: u32; // 推奨されるI/Oで使用するブロックのバイト数 (ex: 4096)
namelen: u32; //ファイル名の最大長
frsize: u32; //最小のブロックのバイト数
reply.statfs(blocks, bfree, bavail, files, ffree, bsize, namelen, frsize);
```

### setxattr
拡張ファイル属性を設定する。
引数は `setxattr(2)` と同様である。

実装しない( `ENOSYS` を返す) 場合、拡張ファイル属性をサポートしない( `ENOTSUP` と同様)と解釈され、
以降カーネルからファイルシステムの呼び出しを行わずに失敗する。

拡張属性は `_name: _value` 形式で与えられる。 

引数の `_position` はmacのリソースフォークで使用されている値で、
基本は0である。osxfuseにのみ存在する引数である。(現在のrustの実装では、mac以外は0を返す)  
rust-fuseでは、 `getxattr` の方に [実装されていないまま](https://github.com/zargony/rust-fuse/issues/40) なので、
とりあえず放置でよいと思われる。

引数の `_flags` には `XATTR_CREATE` または `XATTR_REPLACE` が指定される。  
`XATTR_CREATE` が指定された場合、既に属性が存在する場合は失敗する。  
`XATTR_REPLACE` が指定された場合、属性が存在しない場合失敗する。  
デフォルトでは、属性が存在しない場合作成し、存在する場合は値を置き換える。

### getxattr
拡張ファイル属性を取得する。

引数の `_size` が0の場合、値のサイズを `reply.size()` に入れる。

0でない場合、値のサイズが `_size` 以下の場合、 `reply.data()` に値を入れて返す。  
`_size` を超える場合、 `reply.error(ERANGE)` を返す。

### listxattr
セットされている拡張ファイル属性の名前一覧を得る。

データは、「ヌル終端された文字列が連続して並んでいる」フォーマットである。  
ex: `xxx.data\0yyy.name\0`

引数の `_size` が0の場合、データのサイズを `reply.size()` に入れる。

0でない場合、データのサイズが `_size` 以下の場合、 `reply.data()` にデータを入れて返す。  
`_size` を超える場合、 `reply.error(ERANGE)` を返す。

### removexattr
セットされている拡張ファイル属性を削除する

### access
ファイルのアクセス権を確認する。

引数およびエラーの内容は、 `access(2)` に準ずる。

`access(2)` および `chdir(2)` の時に呼ばれる。
マウントオプションで `default_permissions` が指定されている場合は呼ばれない。

### create
ファイルを作成する。

ファイルが存在しない場合、引数の `_mode` で指定されたモードでファイルを作成し、開く。

`open` と同じ動作のため、open時のフラグが `_flags` で渡される。その他処理は `open` に準ずる。

`create` が実装されていない場合、カーネルは `mknod` と `open` を実行する。

### getlk
POSIXのレコードロックのテストを行う (ロックに関しては `fcntl(2)` の `F_GETLK` を参照)
`_lock_owner: u64, _start: u64, _end: u64, _typ: u32, _pid`

`_typ` は `F_RDLCK, F_WRLCK, F_UNLCK` のいずれかである。

ファイルの該当の範囲がロックできるか調べる。
ユーザのチェックは `_lock_owner` のみで行い、 `pid` は値を返す目的でのみ使用すること。

### setlk
POSIXのレコードロックを設定/解除する。

なお、この関数を実装しない場合でも、カーネルは自前でロック処理を行ってくれる。  
ネットワーク越しのファイルを操作するなど、ファイルシステム側でロック処理を行う必要がある場合に実装する。

`_ino: u64, _fh: u64, _lock_owner: u64, _start: u64, _end: u64, _typ: u32, _pid: u32, _sleep: bool`

引数の `_sleep` がtrueの場合、 `F_SETLKW` と同等の処理を行う。  
つまり、ロックが解除されるまで待たなければならない。シグナルを受けた場合は終了して `EINTR` を返す。

### bmap
オブジェクト内の論理ブロックをデバイスの物理ブロックに結びつけるために使われる。  
つまり、 inodeとブロックのインデックスを与えると、ファイルシステム全体でどこのブロックを使っているかを返す。

vfsではFIBMAP(ioctl)とswapファイルの操作に使われている。
マウントオプションで `blkdev` を指定した場合(ユーザからはブロックデバイスとして見える)にのみ意味がある。

### mac専用関数
`setvolname` `exchange` `getxtimes` はmacOS専用関数である。  
ドキュメントもあまり無いので無視する。

### 未実装の関数
libfuseにはあるがrust-fuseでは未実装(0.3.1現在)の関数一覧を記す。

`ioctl` `poll` `write_buf` `retrieve_reply` `forget_multi` `flock` `fallocate` `readdirplus` `copy_file_range`

## マウント
サンプルの `HelloFS` を参考にマウントする。

```

struct SqliteFS;

impl Filesystem for SqliteFS {
    //ここに実装
}

fn main() {
    let mountpoint = env::args_os().nth(1).unwrap(); // 引数からマウントポイントを取得
    let options = ["-o", "fsname=sqlitefs"]
        .iter()
        .map(|o| o.as_ref())
        .collect::<Vec<&OsStr>>(); // マウントオプションを設定
    fuse::mount(SqliteFS, mountpoint, &options).unwrap();
}
```
