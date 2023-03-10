use fuser::{
    FileAttr, FileType, Filesystem, MountOption, ReplyAttr, ReplyData, ReplyDirectory, ReplyEntry,
    Request,
};
use libc::ENOENT;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::time::{Duration, UNIX_EPOCH};

const TTL: Duration = Duration::from_secs(1); // 1 second

const ROOT_DIR_ATTR: FileAttr = FileAttr {
    ino: 1,
    size: 0,
    blocks: 0,
    atime: UNIX_EPOCH, // 1970-01-01 00:00:00
    mtime: UNIX_EPOCH,
    ctime: UNIX_EPOCH,
    crtime: UNIX_EPOCH,
    kind: FileType::Directory,
    perm: 0o755,
    nlink: 2,
    uid: 502,
    gid: 20,
    rdev: 0,
    flags: 0,
    blksize: 512,
};

struct FS {
    lookup_table: HashMap<String, FileAttr>,
    data_table: HashMap<u64, Vec<u8>>,
    path_table: HashMap<u64, String>,
    last_inode: u64,
}

impl Default for FS {
    fn default() -> Self {
        let mut fs = FS {
            lookup_table: HashMap::new(),
            data_table: HashMap::new(),
            path_table: HashMap::new(),
            last_inode: 1,
        };

        fs.lookup_table.insert(".".to_string(), ROOT_DIR_ATTR);
        fs.path_table.insert(0, ".".to_string());

        fs
    }
}

impl FS {
    fn add_file(&mut self, name: &str, data: &[u8]) -> (u64, FileAttr) {
        let new_inode = self.last_inode + 1;
        let attr = FileAttr {
            ino: new_inode,
            size: data.len() as u64,
            blocks: (data.len() as u64 / 512) + 1,
            atime: UNIX_EPOCH,
            mtime: UNIX_EPOCH,
            ctime: UNIX_EPOCH,
            crtime: UNIX_EPOCH,
            kind: FileType::RegularFile,
            perm: 0o755,
            nlink: 2,
            uid: 501,
            gid: 20,
            rdev: 0,
            flags: 0,
            blksize: 512,
        };

        self.lookup_table.insert(name.to_string(), attr);
        self.data_table.insert(new_inode, data.to_vec());
        self.path_table.insert(new_inode, name.to_string());

        self.last_inode = new_inode;

        (new_inode, attr)
    }

    fn update_fs_size(&mut self) {
        let mut size = 0;

        for v in self.lookup_table.values() {
            if let Some(data) = self.data_table.get(&v.ino) {
                size += data.len();
            }
        }

        self.lookup_table.insert(
            ".".to_string(),
            FileAttr {
                size: size as u64,
                blocks: (size as u64 / 512) + 1,
                ..*self.lookup_table.get(".").unwrap()
            },
        );
    }
}

impl Filesystem for FS {
    fn lookup(&mut self, _req: &Request, parent: u64, name: &OsStr, reply: ReplyEntry) {
        if parent != 1 {
            reply.error(ENOENT);
            return;
        }

        let Some(attr) = self.lookup_table.get(name.to_str().unwrap()) else {
            reply.error(ENOENT);
            return;
        };

        reply.entry(&TTL, attr, 0)
    }

    fn getattr(&mut self, _req: &Request, ino: u64, reply: ReplyAttr) {
        for v in self.lookup_table.values() {
            if v.ino == ino {
                reply.attr(&TTL, v);

                return;
            }
        }

        reply.error(ENOENT);
    }

    fn read(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        _size: u32,
        _flags: i32,
        _lock: Option<u64>,
        reply: ReplyData,
    ) {
        let Some(data) = self.data_table.get(&ino) else {
            reply.error(ENOENT);
            return;
        };

        let data = &data.as_slice()[offset as usize..];

        reply.data(data);
    }

    fn readdir(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        if ino != 1 {
            reply.error(ENOENT);
            return;
        }

        let mut entries: Vec<(u64, FileType, &str)> = vec![(1, FileType::Directory, "..")];

        for (k, v) in &self.lookup_table {
            entries.append(&mut vec![(v.ino, FileType::RegularFile, k.as_str())]);
        }

        println!("{:?}", entries);

        for (i, entry) in entries.into_iter().enumerate().skip(offset as usize) {
            // i + 1 means the index of the next entry
            if reply.add(entry.0, (i + 1) as i64, entry.1, entry.2) {
                break;
            }
        }
        reply.ok();
    }

    fn mknod(
        &mut self,
        _req: &Request<'_>,
        _parent: u64,
        name: &OsStr,
        _mode: u32,
        _umask: u32,
        _rdev: u32,
        reply: ReplyEntry,
    ) {
        let (_, attr) = self.add_file(name.to_str().unwrap(), &[0]);

        reply.entry(&TTL, &attr, 0)
    }

    fn unlink(&mut self, _req: &Request<'_>, _parent: u64, name: &OsStr, reply: fuser::ReplyEmpty) {
        let Some(_) = self.lookup_table.remove(name.to_str().unwrap()) else {
            reply.error(ENOENT);
            return;
        };

        reply.ok();
    }

    fn open(&mut self, _req: &Request<'_>, _ino: u64, _flags: i32, reply: fuser::ReplyOpen) {
        if !self.data_table.contains_key(&_ino) {
            reply.error(ENOENT);

            return;
        }

        self.update_fs_size();

        reply.opened(0, _flags as u32);
    }

    fn write(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        offset: i64,
        data: &[u8],
        _write_flags: u32,
        _flags: i32,
        _lock_owner: Option<u64>,
        reply: fuser::ReplyWrite,
    ) {
        let Some(path) = self.path_table.get(&ino) else {
            reply.error(ENOENT);
            return;
        };

        let Some(attrs) = self.lookup_table.get_mut(path) else {
            reply.error(ENOENT);
            return;
        };

        let Some(existing_data) = self.data_table.get_mut(&ino) else {
            reply.error(ENOENT);
            return;
        };

        let size = data.len();

        for (i, b) in data.iter().enumerate() {
            existing_data.insert(offset as usize + i, *b);
        }

        if data.len() + offset as usize > attrs.size as usize {
            attrs.size = (data.len() + offset as usize) as u64;
        }

        self.update_fs_size();

        reply.written(size as u32);
    }

    fn flush(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _fh: u64,
        _lock_owner: u64,
        reply: fuser::ReplyEmpty,
    ) {
        if !self.data_table.contains_key(&ino) {
            reply.error(ENOENT);

            return;
        }

        self.update_fs_size();

        reply.ok();
    }

    fn release(
        &mut self,
        _req: &Request<'_>,
        _ino: u64,
        _fh: u64,
        _flags: i32,
        _lock_owner: Option<u64>,
        _flush: bool,
        reply: fuser::ReplyEmpty,
    ) {
        if !self.data_table.contains_key(&_ino) {
            reply.error(ENOENT);

            return;
        }

        self.update_fs_size();

        reply.ok();
    }

    fn setattr(
        &mut self,
        _req: &Request<'_>,
        ino: u64,
        _mode: Option<u32>,
        _uid: Option<u32>,
        _gid: Option<u32>,
        _size: Option<u64>,
        _atime: Option<fuser::TimeOrNow>,
        _mtime: Option<fuser::TimeOrNow>,
        _ctime: Option<std::time::SystemTime>,
        _fh: Option<u64>,
        _crtime: Option<std::time::SystemTime>,
        _chgtime: Option<std::time::SystemTime>,
        _bkuptime: Option<std::time::SystemTime>,
        _flags: Option<u32>,
        reply: ReplyAttr,
    ) {
        let path = &self.path_table[&ino];
        let attr = self.lookup_table[path];
        reply.attr(&TTL, &attr);
    }
}

fn main() {
    let mut options = vec![
        MountOption::RW,
        MountOption::FSName("discordfs".to_string()),
    ];
    options.push(MountOption::AutoUnmount);
    options.push(MountOption::AllowOther);

    let mut fs = FS::default();

    fs.add_file("hello.txt", "Hello, World!".as_bytes());
    fs.add_file("amongus.txt", "YOOO I DID IT LETS GOOO".as_bytes());

    fuser::mount2(fs, "./discordfs", &options).unwrap();
}
