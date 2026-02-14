pub mod mdict;
pub mod file_reader;
pub mod header;
pub mod key_index;
mod compressed_block;
mod records;
pub mod format;
mod shared_macros;

pub mod types;
pub mod error;
 
pub use mdict::Mdict;

fn read_int_from_filehandler(
    file_handler: &mut file_reader::FileHandler,
    offset: &mut u64,
    size: usize,
) -> u64 {
    let mut buf = vec![0; size];
    file_handler.read_from_file(*offset, &mut buf).unwrap();
    *offset += size as u64;

    match size {
        4 => shared_macros::read_int_from_buf_u32!(buf, 0) as u64,
        8 => shared_macros::read_int_from_buf_u64!(buf, 0),
        _ => panic!("Invalid buffer size"),
    }
}