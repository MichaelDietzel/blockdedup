# blockdedupe

## What does it do?
Blockdedupe does block-aligned deduplication on XFS and other file systems supporting the FIDEDUPERANGE ioctl.

Blockdedupe parses all files in a given directory and creates checksums for 4kiB-aligned blocks. Then starting from these blocks it checks in both directions for the longes possible duplicate. If there is one of at least 64kiB length (to reduce fragmentation) it is deduplicated. The checks for duplicates are done on the real data and not only on the checksums. Also the kernel repeats that check when using the FIDEDUPERANGE ioctl.
Blocks that do contain only zeros are skipped, as I prefer `fallocate -v --dig-holes <file_name>` for handling them.

## What is data deduplication?

### block based vs chunk based

### online vs offline

## how do I use it?
1. Install rust
2. `cargo build --release`
3. (optional) just seach for duplicate blocks but do not deduplicate them: `./target/release/blockdedupe --simulate /dir/to/dedup`
4. search for duplicate blocks and deduplicate them: `./target/release/blockdedupe /dir/to/dedup`
Some additional hints
- make sure not to write to your file system (or have locked files) while the deduplication is running, that will probably make it abort and you have to run it again. The current implementation just has very limited error handling, a better solution would be possible.
- defragmentation after deduplication can revert the deduplication

## Is it stable to use?
Blockdedupe currently is in a very early stage and it is my first rust program, so there might be obvious errors. I'd be happy for hints on how to improve it.

I am somewhat confident it will not lead to data loss, but obviously I cannot be sure. So backup your data first. Also I have have only done limited testing and only on XFS so there may be many undetected bugs that may lead to crashes.

## how to get good space savings

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