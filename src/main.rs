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


#[derive(Clone)]
struct Blockinfo
{
    crc: u64,
    block_number_plus_one: u64,
}

fn main() -> std::io::Result<()>
{
    println!("starting blockdedupe");

    let mut array: [u8; 4096] =  [0; 4096];

    let file_path: String = String::from("test");

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
    }


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
