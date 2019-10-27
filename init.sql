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

