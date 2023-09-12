# blockdedupe

## What does it do?
Blockdedupe does block-aligned deduplication on XFS and other file systems supporting the FIDEDUPERANGE ioctl.

Blockdedupe parses all files in a given directory and creates checksums for 4kiB-aligned blocks. Then starting from these blocks it checks in both directions for the longes possible duplicate. If there is one of at least 64kiB length (to reduce fragmentation) it is deduplicated. The checks for duplicates are done on the real data and not only on the checksums. Also the kernel repeats that check when using the FIDEDUPERANGE ioctl.
Blocks that do contain only zeros are skipped, as I prefer `fallocate -v --dig-holes <file_name>` for handling them.

## What is data deduplication?
Deduplication is pretty much like reflink-copying data. Identical data blocks that are used in completely different files are just present once on the underlying storage. The file system keeps track of this in its metadata so it can do copy on write when writing to such a block. That way writing to one file doesn't change other ones sharing the same data. Obviously the space savings for this block are gone when that happens.

As this is a process that only affects big blocks of data globally, local reduncancies in the data are left untouched. This means that additional traditional file system compression can be combined with deduplication to get even more space savings. Just make sure to do the compression below the deduplication, otherwise the deduplication will not be able to find matching data properly.

### block-aligned chunking vs content-aligned chunking
Block-aligned chunking just calculates the checksums of data blocks at fixed positions and compares them. So it is important for identical data to also have identical alignment in their blocks so that the duplicates can be found. However some block-aligned deduplication implementations I considered before writing this code took some design decisions were there can be many cases where the alignment just does not work out correctly.

Content-aligned chunking does not depend on the alignment of the data, as it calculates the borders of the chunks based on the data. For this usually a rolling hash over a small buffer that is a lot smaller than the intended chunk size is used. One great tool that uses content-aligned chunking is borg backup. I really like it for my backups, however I cannot currently recommend using it as a file system for performance reasons. I have tried. Also you can only mount it readonly.

Blockdedupe uses a 4kiB alignment and block size because most file systems typically don't have an alignment < 4kiB. With this approach all multiples of 4kiB work fine as well. However this still does not handle cases when you have for example two mostly identical text files where you add a single character in the beginning of one of them. Now the data are no longer 4kiB aligned. Still having the restriction of 4kiB alignment is the only way I could implement it (without writing my own fuse-filesystem or something like that) because all the file systems I could work with do not allow me to deduplicate at arbitrary alignment.

### online vs offline
Maybe in-line vs post-process is the more common wording? However I am used to online vs offline.
Online deduplication happens directly on writing the data, even before they hit the disk. That way duplicate data will not need to be written to disk and instead just a metadata write needs to be done. While this sounds great at first the downside is the huge memory consumption this induces due to having to keep all the hashes of the data in memory for being able to quickly find the duplicates. Although with ssds also lookup from disk maybe could be feasible? I'dont really know.

Offline deduplication on the other hand is a postprocessing operation. The hashes for finding matchin data only need to be kept im memory while the deduplication is performed. Once that is done the runtime overhead is just the additional fragmentation of the file system compared to no deduplication.

Blockdedupe implements offline deduplication. Afaik real online deduplication is not possible using just the FIDEDUPERANGE ioctl.

## how do I use it?
1. Install rust
2. `cargo build --release`
3. (optional) just seach for duplicate blocks but do not deduplicate them: `./target/release/blockdedupe --simulate /dir/to/dedup`
4. search for duplicate blocks and deduplicate them: `./target/release/blockdedupe /dir/to/dedup`
Some additional hints:
- aborting Blockdedupe at any time should be safe
- only run it on a single file system at a time. Currently Blockdedupe follows symlinks and bind mounts, so it doesn't take care of this itself. It will just run for an unnecessary amount of time and then fail do deduplicate.
- make sure not to write to your file system (or have locked files) while the deduplication is running, that will probably make it abort and you have to run it again. The current implementation just has very limited error handling, a better solution would be possible.
- defragmentation after deduplication can revert the deduplication

## Is it stable to use?
Blockdedupe currently is in a very early stage and it is my first rust program, so there might be obvious errors. I'd be happy for hints on how to improve it.

I am somewhat confident it will not lead to data loss, but obviously I cannot be sure. So backup your data first. Also I have have only done limited testing and only on XFS so there may be many undetected bugs that may lead to crashes.

## how to get good space savings
TODO. there is a lot to know.

## Future plans:
- better cli where more options can be selected
- better output
- make sure to not pass file system boundaries
- many optimizations
    - try to reduce fragmentation
    - allow deduplicating small files
    - do not attempt to re-deduplicate already deduplicated blocks
    - skip deduplication for highly compressible blocks? (I use XFS on zfs with compression)
- tests
- test on ZFS as soon as it supports the FIDEDUPERANGE ioctl

## Why another deduplication tool?
I just wanted a simple and reasonably fast program to deduplicate my vm-images on XFS. However other deduplication tools that I could find didn't satisfy my needs so I decided to write my own and learn some rust while doing that.