//! Index Node for file or directory

use core::fmt;
use std::io::Write;

use deku::bitvec::{BitVec, Msb0};
use deku::prelude::*;

use crate::data::DataSize;
use crate::dir::DirectoryIndex;
use crate::entry::Entry;
use crate::kind::Kind;
use crate::metadata::MetadataWriter;
use crate::squashfs::SuperBlock;

#[derive(Debug, DekuRead, DekuWrite, Clone, PartialEq, Eq)]
#[deku(ctx = "bytes_used: u64, block_size: u32, block_log: u16, type_endian: deku::ctx::Endian")]
#[deku(endian = "type_endian")]
#[deku(bit_order = "lsb")]
pub struct Inode {
    pub id: InodeId,
    pub header: InodeHeader,
    #[deku(ctx = "*id, bytes_used, block_size, block_log")]
    pub inner: InodeInner,
}

impl Inode {
    pub fn new(id: InodeId, header: InodeHeader, inner: InodeInner) -> Self {
        Inode { id, header, inner }
    }

    /// Write to `m_writer`, creating Entry
    pub(crate) fn to_bytes<'a>(
        &self,
        name: &'a [u8],
        m_writer: &mut MetadataWriter,
        superblock: &SuperBlock,
        kind: &Kind,
    ) -> Entry<'a> {
        let mut bytes = BitVec::<u8, Msb0>::new();
        self.write(
            &mut bytes,
            (
                0xffff_ffff_ffff_ffff, // bytes_used is unused for ctx. set to max
                superblock.block_size,
                superblock.block_log,
                kind.inner.type_endian,
            ),
        )
        .unwrap();
        let start = m_writer.metadata_start;
        let offset = m_writer.uncompressed_bytes.len() as u16;
        m_writer.write_all(bytes.as_raw_slice()).unwrap();

        Entry {
            start,
            offset,
            inode: self.header.inode_number,
            t: self.id,
            name_size: name.len() as u16 - 1,
            name,
        }
    }
}

#[derive(Debug, DekuRead, DekuWrite, Clone, Copy, PartialEq, Eq)]
#[deku(type = "u16", bits = "4")]
#[deku(endian = "endian", bit_order = "order", ctx = "endian: deku::ctx::Endian, order: deku::ctx::Order")]
#[rustfmt::skip]
pub enum InodeId {
    BasicDirectory       = 1,
    BasicFile            = 2,
    BasicSymlink         = 3,
    BasicBlockDevice     = 4,
    BasicCharacterDevice = 5,
    ExtendedDirectory    = 8,
    ExtendedFile         = 9,
    // TODO:
    // Extended Symlink = 10
    // Extended Block Device = 11
    // Extended Character Device = 12
    // Extended Named Pipe (FIFO) = 13
    // Extended Socked = 14
}

impl InodeId {
    pub(crate) fn into_base_type(self) -> Self {
        match self {
            Self::ExtendedDirectory => InodeId::BasicDirectory,
            Self::ExtendedFile => InodeId::BasicFile,
            _ => self,
        }
    }
}

#[derive(Debug, DekuRead, DekuWrite, Clone, PartialEq, Eq)]
#[deku(
    ctx = "endian: deku::ctx::Endian, order: deku::ctx::Order, id: InodeId, bytes_used: u64, block_size: u32, block_log: u16",
    endian = "endian",
    bit_order = "order",
    id = "id"
)]
pub enum InodeInner {
    #[deku(id = "InodeId::BasicDirectory")]
    BasicDirectory(BasicDirectory),

    #[deku(id = "InodeId::BasicFile")]
    BasicFile(#[deku(ctx = "block_size, block_log")] BasicFile),

    #[deku(id = "InodeId::BasicSymlink")]
    BasicSymlink(BasicSymlink),

    #[deku(id = "InodeId::BasicBlockDevice")]
    BasicBlockDevice(BasicDeviceSpecialFile),

    #[deku(id = "InodeId::BasicCharacterDevice")]
    BasicCharacterDevice(BasicDeviceSpecialFile),

    #[deku(id = "InodeId::ExtendedDirectory")]
    ExtendedDirectory(ExtendedDirectory),

    #[deku(id = "InodeId::ExtendedFile")]
    ExtendedFile(#[deku(ctx = "bytes_used, block_size, block_log")] ExtendedFile),
}

#[derive(Debug, DekuRead, DekuWrite, Clone, Copy, PartialEq, Eq, Default)]
#[deku(
    ctx = "endian: deku::ctx::Endian, order: deku::ctx::Order",
    endian = "endian",
    bit_order = "order"
)]
pub struct InodeHeader {
    #[deku(bits = "12")]
    pub permissions: u16,
    /// index into id table
    #[deku(bits = "8")]
    pub uid: u16,
    /// index into id table
    #[deku(bits = "8")]
    pub gid: u16,
    pub mtime: u32,
    pub inode_number: u32,
}

#[derive(Debug, DekuRead, DekuWrite, Clone, PartialEq, Eq)]
#[deku(
    ctx = "endian: deku::ctx::Endian, order: deku::ctx::Order",
    endian = "endian",
    bit_order = "order"
)]
pub struct BasicDirectory {
    pub block_index: u32,
    pub link_count: u32,
    #[deku(bits = "19")]
    pub file_size: u16,
    #[deku(bits = "13")]
    pub block_offset: u16,
    pub parent_inode: u32,
}

#[derive(Debug, DekuRead, DekuWrite, Clone, PartialEq, Eq)]
#[deku(
    ctx = "endian: deku::ctx::Endian, order: deku::ctx::Order",
    endian = "endian",
    bit_order = "order"
)]
pub struct ExtendedDirectory {
    pub link_count: u32,
    #[deku(bits = "27")]
    pub file_size: u32,
    #[deku(bits = "13")]
    pub block_offset: u32,
    pub start_block: u32,
    #[deku(assert = "*i_count < 256")]
    pub i_count: u16,
    pub parent_inode: u32,
    #[deku(count = "*i_count")]
    pub dir_index: Vec<DirectoryIndex>,
}

#[derive(Debug, DekuRead, DekuWrite, Clone, PartialEq, Eq)]
#[deku(
    ctx = "endian: deku::ctx::Endian, order: deku::ctx::Order, block_size: u32, block_log: u16",
    endian = "endian",
    bit_order = "order"
)]
pub struct BasicFile {
    pub blocks_start: u32,
    pub frag_index: u32,
    pub block_offset: u64,
    #[deku(
        bytes = "4",
        assert = "((*file_size as u128) < byte_unit::n_tib_bytes(1))"
    )]
    pub file_size: u64,
    #[deku(count = "block_count(block_size, block_log, *frag_index, *file_size as u64)")]
    pub block_sizes: Vec<DataSize>,
}

impl From<&ExtendedFile> for BasicFile {
    fn from(ex_file: &ExtendedFile) -> Self {
        Self {
            blocks_start: ex_file.blocks_start as u32,
            frag_index: ex_file.frag_index,
            block_offset: ex_file.block_offset,
            file_size: ex_file.file_size as u32,
            block_sizes: ex_file.block_sizes.clone(),
        }
    }
}

#[derive(Debug, DekuRead, DekuWrite, Clone, PartialEq, Eq)]
#[deku(
    ctx = "endian: deku::ctx::Endian, order: deku::ctx::Order, bytes_used: u32, block_size: u32, block_log: u16",
    endian = "endian",
    bit_order = "order"
)]
pub struct ExtendedFile {
    pub blocks_start: u64,
    #[deku(
        assert = "((*file_size as u128) < byte_unit::n_tib_bytes(1)) && (*file_size < bytes_used)"
    )]
    pub file_size: u64,
    pub sparse: u64,
    pub link_count: u32,
    pub frag_index: u32,
    pub block_offset: u32,
    pub xattr_index: u32,
    #[deku(count = "block_count(block_size, block_log, *frag_index, *file_size)")]
    pub block_sizes: Vec<DataSize>,
}

fn block_count(block_size: u32, block_log: u16, fragment: u32, file_size: u64) -> u64 {
    const NO_FRAGMENT: u32 = 0xffffffff;

    if fragment == NO_FRAGMENT {
        (file_size + u64::from(block_size) - 1) >> block_log
    } else {
        file_size >> block_log
    }
}

#[derive(DekuRead, DekuWrite, Clone, PartialEq, Eq)]
#[deku(
    ctx = "endian: deku::ctx::Endian, order: deku::ctx::Order",
    endian = "endian",
    bit_order = "order"
)]
pub struct BasicSymlink {
    pub link_count: u32,
    #[deku(assert = "*target_size < 256")]
    pub target_size: u32,
    #[deku(count = "target_size")]
    pub target_path: Vec<u8>,
}

impl fmt::Debug for BasicSymlink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BasicSymlink")
            .field("link_count", &self.link_count)
            .field("target_size", &self.target_size)
            .field("target_path", &self.target())
            .finish()
    }
}
impl BasicSymlink {
    pub fn target(&self) -> String {
        std::str::from_utf8(&self.target_path).unwrap().to_string()
    }
}

#[derive(Debug, DekuRead, DekuWrite, Clone, PartialEq, Eq)]
#[deku(
    ctx = "endian: deku::ctx::Endian, order: deku::ctx::Order",
    endian = "endian",
    bit_order = "order"
)]
pub struct BasicDeviceSpecialFile {
    pub link_count: u32,
    pub device_number: u16,
}
