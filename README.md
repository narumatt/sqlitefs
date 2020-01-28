# sqlite-fs

## about

sqlite-fs allows Linux and MacOS to mount a sqlite database file as a normal filesystem.

## Requirements

- Latest Rust Programming Language (≥ 1.38)
- libfuse(Linux) or osxfuse(MacOS) is requied by [fuse-rs](https://github.com/zargony/fuse-rs)

## Usage
### Create database

```
$ sqlite3 ~/filesystem.sqlite < init.sql
$ sqlite3 ~/filesystem.sqlite < hello.sql
```

### Mount a filesystem

```
$ sqlite-fs <mount_point> <db_path>
```

### Unmount a filesystem

- Linux

```
$ fusermount -u <mount_point>
```

## example
```
$ sqlite-fs ~/mount ~/filesystem.sqlite &
$ cat ~/mount/hello.txt
Hello world!
```

