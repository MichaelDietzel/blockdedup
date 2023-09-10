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
use std::os::unix::io::{RawFd, AsRawFd};
use errno::errno;
use std::os::raw::{c_int, c_ulong};
use clap::Parser;

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

    let mut array: [u8; 4096] =  [0; 4096];

    let file_path: String = args.path.into_os_string().into_string().unwrap();

    let file = File::open(&file_path)?;

    let file_size: u64 = file.metadata().unwrap().len();
    println!("file size: {}", file_size);

    let block_count: u64 = file_size / 4096 + 1;
    let block_count_usize: usize = usize::try_from(block_count).unwrap();
    println!("block count: {}", block_count);

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
                    let block_number_old = hashes[hash_index].block_number_plus_one - 1;
                    let matchlen: u64 = check_match(&file_path, block_number_old, block_number, block_count-1)?;
                    println!("found match for block #{} at block #{}. Matchlen: {} blocks.", block_number, block_number_old, matchlen);
                    if matchlen > 0
                    {
                        matches += 1;
                        total_matchsize += matchlen;
                        skip_match_check = matchlen - 1;

                        if matchlen >= 16
                        {

                            if !args.simulate
                            {
                                let file_src = File::open(&file_path)?;
                                let src_fd: RawFd = file_src.as_raw_fd();
                                let file_dest = File::options().write(true).open(&file_path)?;
                                let dest_fd: RawFd = file_dest.as_raw_fd();
                                let dest_fd_i64: i64 = dest_fd as i64;

                                let mut dedup_request: dedup = dedup
                                {
                                    args: file_dedupe_range
                                    {
                                        src_offset: block_number_old*4096,
                                        src_length: matchlen*4096,
                                        dest_count: 1,
                                        reserved1: 0,
                                        reserved2: 0,
                                    },
                                    info: file_dedupe_range_info
                                    {
                                        dest_fd: dest_fd_i64,
                                        dest_offset: block_number*4096,
                                        bytes_deduped: 0,
                                        status: 0,
                                        reserved: 0,
                                    },
                                };
                                let req = &mut dedup_request;

                                let result: i32;
                                unsafe
                                {
                                    result = ioctl(src_fd, FIDEDUPERANGE, req as *mut _);
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
                        }
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

fn check_match(file_path: &String, block_old: u64, block_new: u64, full_block_count: u64) -> std::io::Result<u64>
{
    let file_old = File::open(&file_path)?;
    let file_new = File::open(&file_path)?;

    let mut buf_old: [u8; 4096] = [0; 4096];
    let mut buf_new: [u8; 4096] = [0; 4096];

    let mut reader_old = BufReader::new(file_old);
    let mut reader_new = BufReader::new(file_new);

    reader_old.seek(SeekFrom::Start(block_old * 4096))?;
    reader_old.read(&mut buf_old[..])?;

    reader_new.seek(SeekFrom::Start(block_new * 4096))?;
    reader_new.read(&mut buf_new[..])?;

    let crc = Crc::<u64>::new(&CRC_64_ECMA_182);

    if buf_old != buf_new
    {
        let mut digest_old = crc.digest();
        digest_old.update(&buf_old);
        let _crc_old: u64 = digest_old.finalize();

        let mut digest_new = crc.digest();
        digest_new.update(&buf_new);
        let _crc_new: u64 = digest_new.finalize();


        println!("match could not be confirmed when reading real data");
        return Ok(0);
    }

    let mut blocks_before: u64 = 0;

    let matchlen_max: u64 = block_new-block_old; //in blocks

    //match blocks before the first matching block found by its hash. only relevant in case of a hash collisions
    /*
    TODO: this cannot be used currently because the function calling this relies on the match starting at the passed position.
    let mut max_blocks_before: u64 = matchlen_max;
    if max_blocks_before > block_old-1
    {
        max_blocks_before = block_old-1;
    }

    for block_offset in (1..max_blocks_before).rev()
    {
        reader_old.seek(SeekFrom::Start((block_old - block_offset) * 4096))?;
        reader_old.read(&mut buf_old[..])?;

        let mut digest = crc.digest();
        digest.update(&buf_old);
        let crc_result: u64 = digest.finalize();
        if crc_result == 0
        {
            break;
        }

        reader_new.seek(SeekFrom::Start((block_new - block_offset) * 4096))?;
        reader_new.read(&mut buf_new[..])?;

        if buf_old == buf_new
        {
            blocks_before = block_offset;
        }
        else
        {
            break;
        }
    }*/


    //find additional matching blocks after the matching block found by its hash.
    let mut blocks_after: u64 = 0;

    reader_old.seek(SeekFrom::Start((block_old + 1) * 4096))?;
    reader_new.seek(SeekFrom::Start((block_new + 1)* 4096))?;

    let mut max_blocks_after: u64 = matchlen_max-blocks_before;
    let remaining_blocks: u64 = full_block_count - block_new;
    if max_blocks_after > remaining_blocks
    {
        max_blocks_after = remaining_blocks;
    }


    for block_offset in 1..(max_blocks_after)
    {
        reader_old.read(&mut buf_old[..])?;

        let mut digest = crc.digest();
        digest.update(&buf_old);
        let crc_result: u64 = digest.finalize();
        if crc_result == 0
        {
            break;
        }

        reader_new.read(&mut buf_new[..])?;

        if buf_old == buf_new
        {
            blocks_after = block_offset;
        }
        else
        {
            break;
        }
    }

    return Ok(blocks_before + 1 + blocks_after);

}
