# sqlite-fs

## About

sqlite-fs allows Linux and MacOS to mount a sqlite database file as a normal filesystem.

## Requirements

- Latest Rust Programming Language (≥ 1.38)
- libfuse(Linux) osxfuse(MacOS) requied by [rust-fuse](https://github.com/zargony/rust-fuse)

## Usage
### Mount a filesystem

```
$ sqlite-fs <mount_point> <db_path>
```

If a database file doesn't exist, sqlite-fs create db and tables.

### Unmount a filesystem

- Linux

```
$ fusermount -u <mount_point>
```

- Mac

```
$ umount <mount_point>
```

## example
```
$ sqlite-fs ~/mount ~/filesystem.sqlite &
$ echo "Hello world\!" > ~/mount/hello.txt
$ cat ~/mount/hello.txt
Hello world!
```

