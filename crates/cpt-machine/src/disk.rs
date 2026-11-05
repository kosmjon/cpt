//! File-backed data disk with two drive stations (A and B).
//!
//! The real CPT had dual 8-inch floppy drives. We do not emulate floppy hardware; instead each
//! **drive station** is backed by a host file divided into fixed-size blocks (sectors). The
//! CPU reaches the drives through a controller [`Device`] (see `DiskController`).
//!
//! Two layers, with a clear line between "real" and "placeholder":
//!   * [`Drive`] / [`DiskImage`] — the backing store (read/write/flush blocks to a host file).
//!     This is real and correct regardless of the controller wiring.
//!   * [`DiskController`] — exposes the drives to the CPU via I/O ports using a PLACEHOLDER
//!     command protocol, isolated so the real protocol (from the B-tree filer source) is a
//!     localized change. [VERIFY]
//!
//! The two stations (shown to the user as **drive 1 and drive 2**) are **symmetric data
//! stations**: both support read/write/flush and can be mounted/ejected at runtime (the
//! operator could pull the program disk after boot and copy / re-paginate files between the two
//! in either direction). The system itself is loaded from a linked image, not booted from a
//! disk, so neither station is special to the hardware.
//!
//! Geometry (block size, block count) is configurable; defaults are placeholders. [VERIFY]

use std::io::{self, Read, Seek, SeekFrom, Write};

/// Which drive station. The two stations are presented to the **user as drive 1 and drive 2**;
/// `A`/`B` are just internal identifiers (A = drive 1, B = drive 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Station {
    /// Drive 1 (shown to the user as "1").
    A,
    /// Drive 2 (shown to the user as "2").
    B,
}

/// Disk geometry. Block size and count are [VERIFY] placeholders until the filer source is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Geometry {
    /// Bytes per block (sector). Default 128 (classic 8-inch IBM-3740 sector). [VERIFY]
    pub block_size: usize,
    /// Number of blocks on the disk. Default 2002 (~250K at 128 bytes). [VERIFY]
    pub block_count: usize,
}

impl Default for Geometry {
    fn default() -> Self {
        Geometry {
            block_size: 128,
            block_count: 2002,
        }
    }
}

impl Geometry {
    pub fn total_bytes(&self) -> usize {
        self.block_size * self.block_count
    }
}

/// A disk image: either in-memory (tests / unattached) or backed by a host file.
pub struct DiskImage {
    geom: Geometry,
    data: Vec<u8>,
    /// If present, writes are flushed through to this file.
    backing: Option<std::fs::File>,
    dirty: bool,
}

impl DiskImage {
    /// A blank in-memory image of the given geometry (no host file).
    pub fn blank(geom: Geometry) -> Self {
        DiskImage {
            data: vec![0; geom.total_bytes()],
            geom,
            backing: None,
            dirty: false,
        }
    }

    /// Open (or create) a host file as the backing store. The file is sized to the geometry:
    /// a shorter file is zero-padded; a longer one is truncated to the disk size.
    pub fn open_file(path: &str, geom: Geometry) -> io::Result<Self> {
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(path)?;
        let mut data = Vec::new();
        file.read_to_end(&mut data)?;
        data.resize(geom.total_bytes(), 0);
        Ok(DiskImage {
            geom,
            data,
            backing: Some(file),
            dirty: false,
        })
    }

    pub fn geometry(&self) -> Geometry {
        self.geom
    }

    /// Read a block into `out` (must be `block_size` long). Returns false if the block index is
    /// out of range.
    pub fn read_block(&self, block: usize, out: &mut [u8]) -> bool {
        let bs = self.geom.block_size;
        if block >= self.geom.block_count || out.len() != bs {
            return false;
        }
        let start = block * bs;
        out.copy_from_slice(&self.data[start..start + bs]);
        true
    }

    /// Write a block from `src` (must be `block_size` long). Returns false if out of range.
    pub fn write_block(&mut self, block: usize, src: &[u8]) -> bool {
        let bs = self.geom.block_size;
        if block >= self.geom.block_count || src.len() != bs {
            return false;
        }
        let start = block * bs;
        self.data[start..start + bs].copy_from_slice(src);
        self.dirty = true;
        true
    }

    /// Flush any pending writes to the backing file (no-op for in-memory images).
    pub fn flush(&mut self) -> io::Result<()> {
        if let Some(file) = &mut self.backing {
            if self.dirty {
                file.seek(SeekFrom::Start(0))?;
                file.write_all(&self.data)?;
                file.flush()?;
                self.dirty = false;
            }
        }
        Ok(())
    }

    /// Byte-level accessor (for tests).
    pub fn bytes(&self) -> &[u8] {
        &self.data
    }
}

/// A drive station: holds an optional mounted image.
#[derive(Default)]
pub struct Drive {
    image: Option<DiskImage>,
}

impl Drive {
    pub fn new() -> Self {
        Drive { image: None }
    }

    /// Mount an image in this drive.
    pub fn mount(&mut self, image: DiskImage) {
        self.image = Some(image);
    }

    /// Eject (flushing first). Returns the image so the caller can keep it if desired.
    pub fn eject(&mut self) -> Option<DiskImage> {
        if let Some(img) = &mut self.image {
            let _ = img.flush();
        }
        self.image.take()
    }

    pub fn is_mounted(&self) -> bool {
        self.image.is_some()
    }

    pub fn image_mut(&mut self) -> Option<&mut DiskImage> {
        self.image.as_mut()
    }

    pub fn image(&self) -> Option<&DiskImage> {
        self.image.as_ref()
    }
}
