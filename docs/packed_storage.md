Packed Storage Format
=====================

Overview
--------

Packed storage centralizes the on-disk layout used by both "readings" and "records" outputs into a single header + concatenated compressed blocks format. The implementation lives in `src/mdx_conversion/packed_storage.rs` and is used from:

- `src/mdx_conversion/fst_indexing.rs` (writes readings file)
- `src/mdx_conversion/fst_map.rs` (reads readings file)
- `src/mdx_conversion/records.rs` (writes compacted record file)

Design goals
------------

- Single canonical header (magic + version)
- Explicit encoding id + compression level
- Concatenated compressed blocks with prefix sums for random access
- Blocks are opaque bytes; callers decide how to encode/decode block contents

Byte-level header (little-endian integers)
-----------------------------------------

All offsets below are relative to the file start.

- 0x00..0x07 (8 bytes): ASCII magic `PKGSTRG1`
- 0x08      (1 byte) : version (u8) — current value `1`
- 0x09      (1 byte) : flags (u8) — reserved (must be zero)
- 0x0A..0x0B (2 bytes): reserved (u16) zero
- 0x0C      (1 byte) : encoding_id (u8) — see Encoding IDs
- 0x0D      (1 byte) : compression_level (u8) — 0..=10 (0 = default)
- 0x0E..0x0F (2 bytes): reserved (u16) zero (padding)
- 0x10..0x17 (8 bytes): num_blocks (u64 LE) — number of prefix entries (N)
- 0x18..0x1F (8 bytes): num_entries (u64 LE) — number of packed entries pushed (E)
- Then N entries (16 bytes each):
  - u64 LE compressed_end[i]
  - u64 LE uncompressed_end[i]
- Immediately following header: concatenated compressed blocks (block_0, block_1, ...)

Notes
-----

- `compressed_end[]` values are byte offsets relative to the start of the concatenated block region.
- `uncompressed_end[]` values are offsets in uncompressed bytes; the first pair is typically `(0,0)`.
- `num_entries` is informational and equals the number of entries pushed into the packer (PackedIndex).
- `encoding_id` and `compression_level` are global per-file; blocks must be decoded using the indicated encoding.

Encoding IDs
------------

- 0 = NONE / RAW (no compression)
- 1 = LZO
- 2 = GZIP
- 3 = ZSTD
- 4 = LZ4

Compression level
-----------------

- `compression_level` is u8 0..=10 where 0 means "default" for the encoder.
- For encoders that accept levels (ZSTD), 1..=10 map to encoder levels. Writers should clamp to 10.

Reading records
--------------------------------------
Biary search `uncompressed_end[]` to find the block containing the desired uncompressed offset, then use `compressed_end[]` to locate the compressed bytes and decode using the indicated encoding.

You may not assume the size of records in advanced. Provide a callback for the caller to process decoded blocks and track offsets as needed.

Implementation notes
--------------------

- The new code should be self-contained in `src/packed_storage/` folder with a clean API for writing and reading the packed storage format.
- Optimize for streaming writes and reads; avoid buffering entire contents in memory when possible.
- Write tests in a new file that tests storing example data and reading it back correctly, including edge cases (empty blocks, single block, multiple blocks, etc).

Next steps
----------

1) Run the test suite: `cargo test` (expect to fix minor compiler or lifetime issues).
2) Add unit tests for `packed_storage` (round-trip header + blocks) and small integration tests for readings/records.
3) Implement runtime decoding switch in `fst_map`/records to choose LZO/GZIP/ZSTD/LZ4 decoders when `encoding_id` != ZSTD.

Reference code
---------------

- `src/mdx_conversion/packed_storage.rs` — header read/write helpers and constants
- `src/mdx_conversion/packed_index.rs` — packer producing `PackedResult` used by the storage writer
