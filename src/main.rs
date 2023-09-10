/*
    Blockdedup does block-aligned deduplication on xfs and other
    file systems supporting the FIDEDUPERANGE ioctl.
    Copyright (C) 2023  Michael Dietzel

    This program is free software: you can redistribute it and/or modify
    it under the terms of the GNU General Public License as published by
    the Free Software Foundation, either version 3 of the License, or
    (at your option) any later version.

    This program is distributed in the hope that it will be useful,
    but WITHOUT ANY WARRANTY; without even the implied warranty of
    MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
    GNU General Public License for more details.

    You should have received a copy of the GNU General Public License
    along with this program.  If not, see <https://www.gnu.org/licenses/>.
*/


use std::fs::File;
use std::io::BufReader;
use std::io::SeekFrom;
use std::io::prelude::*;
use crc::{Crc, CRC_64_ECMA_182};
use num_format::ToFormattedString;
use std::os::unix::io::{RawFd, AsRawFd};
use errno::errno;
use std::os::raw::{c_int, c_ulong};
use clap::Parser;
use num_format::SystemLocale;

#[allow(non_camel_case_types)]
#[repr(C)]
pub struct file_dedupe_range_info
{
    dest_fd: i64,
    dest_offset: u64,
    bytes_deduped: u64,
    status: i32,
    reserved: u32,
}

#[allow(non_camel_case_types)]
#[repr(C)]
pub struct file_dedupe_range
{
    src_offset: u64,
    src_length: u64,
    dest_count: u16, //count of elements in the info array
    reserved1: u16, //must be 0
    reserved2: u16, //must be 0
}

#[allow(non_camel_case_types)]
#[repr(C)]
pub struct dedup
{
    args: file_dedupe_range,
    info: file_dedupe_range_info,
}

const FIDEDUPERANGE: c_ulong = 0xC0189436; //copied from c. 0xC0: direction: (_IOC_READ|_IOC_WRITE). 0x18 size of struct file_dedupe_range. 0x94 type (see linux/fs.h). 0x36 number (see linux/fs.h)

extern
{
    fn ioctl(fd: c_int, request: c_ulong, ...) -> c_int;
}


#[derive(Clone)]
struct Blockinfo
{
    crc: u64,
    block_number_plus_one: u64,
}

#[derive(Parser)]
struct CliArgs
{
    /// do not actually deduplicate, just report the matches
    #[arg(short, long, default_value_t = false)]
    simulate: bool,

    /// the file on which the deduplication should be performed
    path: std::path::PathBuf,
}

fn main() -> std::io::Result<()>
{
    let args = CliArgs::parse();

    println!("starting blockdedupe");

    let locale = SystemLocale::default().unwrap();

    let mut array: [u8; 4096] =  [0; 4096];

    let file_path: String = args.path.into_os_string().into_string().unwrap();

    let file = File::open(&file_path)?;

    let file_size: u64 = file.metadata().unwrap().len();
    let file_size_formatted = file_size.to_formatted_string(&locale);
    println!("file size: {} Bytes", file_size_formatted);

    let block_count: u64 = file_size / 4096 + 1;
    let block_count_usize: usize = usize::try_from(block_count).unwrap();
    let block_count_formatted = block_count.to_formatted_string(&locale);
    println!("block count: {}", block_count_formatted);

    let mut hashes: Vec<Blockinfo> = vec![Blockinfo {crc: 0, block_number_plus_one: 0}; block_count_usize];

    let mut buf_reader = BufReader::new(&file);

    let mut matches: u64 = 0;
    let mut total_matchsize: u64 = 0;

    let mut block_number: u64 = 0;
    let mut skip_match_check: u64 = 0;

    let crc = Crc::<u64>::new(&CRC_64_ECMA_182);

    while block_number < block_count
    {
        buf_reader.read(&mut array[..])?;

        let mut digest = crc.digest();

        digest.update(&array);
        let crc_result: u64 = digest.finalize();
        if crc_result != 0
        {
            let hash_index: usize = usize::try_from(crc_result % block_count).unwrap();

            if skip_match_check > 0
            {
                skip_match_check -= 1;
            }
            else
            {
                let hash_old: u64 = hashes[hash_index].crc;

                if hashes[hash_index].block_number_plus_one > 0 && hash_old == crc_result
                {
                    let block_number_keep = hashes[hash_index].block_number_plus_one - 1;
                    let matched_blocks : u64= try_dedupe_match(&file_path, block_number_keep, &file_path, block_number, args.simulate);
                    if matched_blocks > 0
                    {
                        matches += 1;
                        total_matchsize += matched_blocks;
                        skip_match_check = matched_blocks - 1;
                    }
                }
            }

            hashes[hash_index].crc = crc_result;
            hashes[hash_index].block_number_plus_one = block_number+1;
        }
        block_number += 1;
    }
    println!("found {} matches for a total of {} matching blocks", matches, total_matchsize);
    Ok(())
}


fn try_dedupe_match(file_path_keep: &String, block_offset_keep: u64, file_path_dedup: &String, block_offset_dedup: u64, simulate: bool) -> u64
{
    let file_keep = File::open(&file_path_keep).unwrap();
    let file_dedup = File::open(&file_path_dedup).unwrap();

    let mut buf_keep: [u8; 4096] = [0; 4096];
    let mut buf_dedupe: [u8; 4096] = [0; 4096];

    let file_size_keep: u64 = file_keep.metadata().unwrap().len();
    let file_size_dedup: u64 = file_dedup.metadata().unwrap().len();

    let mut reader_keep: BufReader<File> = BufReader::new(file_keep);
    let mut reader_dedup: BufReader<File> = BufReader::new(file_dedup);

    reader_keep.seek(SeekFrom::Start(block_offset_keep * 4096)).unwrap();
    reader_keep.read(&mut buf_keep[..]).unwrap();

    reader_dedup.seek(SeekFrom::Start(block_offset_dedup * 4096)).unwrap();
    reader_dedup.read(&mut buf_dedupe[..]).unwrap();

    let crc = Crc::<u64>::new(&CRC_64_ECMA_182);

    if buf_keep != buf_dedupe
    {
        if cfg!(debug_assertions)
        {
            let mut digest_old = crc.digest();
            digest_old.update(&buf_keep);
            let _crc_old: u64 = digest_old.finalize();

            let mut digest_new = crc.digest();
            digest_new.update(&buf_dedupe);
            let _crc_new: u64 = digest_new.finalize();
        }
        println!("found matching crc for block #{} at block #{}", block_offset_dedup, block_offset_keep);
        println!("match could not be confirmed when reading real data");
        return 0;
    }

    let blocks_before: u64 = find_matching_blocks_before(file_path_keep == file_path_dedup, &mut reader_keep, block_offset_keep, &mut reader_dedup, block_offset_dedup);

    let full_blocks_keep: u64 = file_size_keep / 4096;
    let full_blocks_dedup: u64 = file_size_dedup / 4096;

    let blocks_behind: u64 = find_matching_blocks_behind(file_path_keep == file_path_dedup, &mut reader_keep, block_offset_keep, full_blocks_keep, &mut reader_dedup, block_offset_dedup, full_blocks_dedup, blocks_before);

    let blocks_dedupe_count: u64 = blocks_before + 1 + blocks_behind;

    println!("found match for block #{} at block #{}. Matchlen: {} blocks.", block_offset_dedup, block_offset_keep, blocks_dedupe_count);

    if !simulate && blocks_dedupe_count >= 16
    {
        do_dedupe(file_path_keep, block_offset_keep-blocks_before, file_path_dedup, block_offset_dedup-blocks_before, blocks_dedupe_count);
    }

    return blocks_dedupe_count;

}

fn find_matching_blocks_before(keep_equals_dedup: bool, reader_keep: &mut BufReader<File>, block_offset_keep: u64, reader_dedup: &mut BufReader<File>, block_offset_dedup: u64) -> u64
{
    let mut buf_keep: [u8; 4096] = [0; 4096];
    let mut buf_dedupe: [u8; 4096] = [0; 4096];

    let mut max_blocks_before: u64;
    if block_offset_keep < block_offset_dedup
    {
        max_blocks_before = block_offset_keep;
    }
    else
    {
        max_blocks_before = block_offset_dedup;
    }
    if keep_equals_dedup
    {
        assert!(block_offset_keep < block_offset_dedup);
        let tmp: u64 = block_offset_dedup - block_offset_keep;

        if tmp < max_blocks_before
        {
            max_blocks_before = tmp;
        }
    }

    let crc = Crc::<u64>::new(&CRC_64_ECMA_182);

    for block_offset in 1..max_blocks_before
    {
        reader_keep.seek(SeekFrom::Start((block_offset_keep - block_offset) * 4096)).unwrap();
        reader_keep.read(&mut buf_keep[..]).unwrap();

        let mut digest = crc.digest();
        digest.update(&buf_keep);
        let crc_result: u64 = digest.finalize();
        if crc_result == 0
        {
            return block_offset-1; //do not attempt to match blocks that are completely zero. they could (and probably should) be holes.
        }

        reader_dedup.seek(SeekFrom::Start((block_offset_dedup - block_offset) * 4096)).unwrap();
        reader_dedup.read(&mut buf_dedupe[..]).unwrap();

        if buf_keep != buf_dedupe
        {
            return block_offset-1;
        }
    }

    return max_blocks_before;
}


fn find_matching_blocks_behind(keep_equals_dedup: bool, reader_keep: &mut BufReader<File>, block_offset_keep: u64, full_blocks_keep: u64, reader_dedup: &mut BufReader<File>, block_offset_dedup: u64, full_blocks_dedup: u64, matching_before: u64) -> u64
{
    let mut buf_keep: [u8; 4096] = [0; 4096];
    let mut buf_dedup: [u8; 4096] = [0; 4096];

    let mut max_blocks_behind: u64;
    if full_blocks_keep - block_offset_keep < full_blocks_dedup - block_offset_dedup
    {
        max_blocks_behind = full_blocks_keep - block_offset_keep;
    }
    else
    {
        max_blocks_behind = full_blocks_dedup - block_offset_dedup;
    }
    if keep_equals_dedup
    {
        assert!(block_offset_keep < block_offset_dedup);
        let tmp: u64 = block_offset_dedup - block_offset_keep - matching_before;

        if tmp < max_blocks_behind
        {
            max_blocks_behind = tmp;
        }
    }

    reader_keep.seek(SeekFrom::Start((block_offset_keep + 1) * 4096)).unwrap();
    reader_dedup.seek(SeekFrom::Start((block_offset_dedup + 1)* 4096)).unwrap();

    let crc = Crc::<u64>::new(&CRC_64_ECMA_182);

    for block_offset in 1..max_blocks_behind
    {
        reader_keep.read(&mut buf_keep[..]).unwrap();

        let mut digest = crc.digest();
        digest.update(&buf_keep);
        let crc_result: u64 = digest.finalize();
        if crc_result == 0
        {
            return block_offset - 1;
        }

        reader_dedup.read(&mut buf_dedup[..]).unwrap();

        if buf_keep != buf_dedup
        {
            return block_offset - 1;
        }
    }
    return max_blocks_behind;
}

fn do_dedupe(file_path_keep: &String, block_offset_keep: u64, file_path_dedup: &String, block_offset_dedup: u64, blocks_dedup_count : u64)
{
    let file_keep = File::open(&file_path_keep).unwrap();
    let fd_keep: RawFd = file_keep.as_raw_fd();
    let file_dedup = File::options().write(true).open(&file_path_dedup).unwrap();
    let fd_dedup: RawFd = file_dedup.as_raw_fd();
    let fd_dedup_i64: i64 = fd_dedup as i64;

    let mut dedup_request: dedup = dedup
    {
        args: file_dedupe_range
        {
            src_offset: block_offset_keep*4096,
            src_length: blocks_dedup_count*4096,
            dest_count: 1,
            reserved1: 0,
            reserved2: 0,
        },
        info: file_dedupe_range_info
        {
            dest_fd: fd_dedup_i64,
            dest_offset: block_offset_dedup*4096,
            bytes_deduped: 0,
            status: 0,
            reserved: 0,
        },
    };
    let req = &mut dedup_request;

    let result: i32;
    unsafe
    {
        result = ioctl(fd_keep, FIDEDUPERANGE, req as *mut _);
    }
    if result != 0
    {
        let errno_whatever = errno();
        let errno_i32: i32 = errno_whatever.0;
        println!("dedup error: ({}) {}", errno_i32, errno_whatever);
        panic!("aborting");
    }
    else
    {
        println!("dedup success!");
        println!("bytes_deduped {}", dedup_request.info.bytes_deduped);
        println!("status {}", dedup_request.info.status);
    }
}
