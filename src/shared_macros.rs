// Macro to generate macros for reading integers of different sizes from a buffer and updating the offset
macro_rules! generate_read_int_macros {
    ($($type:ty);*) => {
        $(
            paste::item! {
                macro_rules! [< read_int_from_buf_ $type >] {
                    ($buf:expr, $offset:expr) => {
                        {
                            let size_of_int = std::mem::size_of::<$type>();
                            let int = <$type>::from_be_bytes($buf[$offset..$offset + size_of_int].try_into().unwrap());
                            int
                        }
                    };
                }

                macro_rules! [< read_int_from_buf_le_ $type >] {
                    ($buf:expr, $offset:expr) => {
                        {
                            let size_of_int = std::mem::size_of::<$type>();
                            let int = <$type>::from_le_bytes($buf[$offset..$offset + size_of_int].try_into().unwrap());
                            int
                        }
                    };
                }

                pub(crate) use [< read_int_from_buf_ $type >];
                pub(crate) use [< read_int_from_buf_le_ $type >];
            }
        )*
    };
}

// Generate the specific macros for 1, 2, 4, and 8 bytes
generate_read_int_macros!(
    u8;
    u16;
    u32;
    u64
);

// Increases the offset by the size of the integer and returns the integer
macro_rules! read_int_from_buf {
    ($buf:expr, $offset:expr, $size:expr) => {{
        let value = match $size {
            1 => read_int_from_buf_u8!($buf, $offset) as u64,
            2 => read_int_from_buf_u16!($buf, $offset) as u64,
            4 => read_int_from_buf_u32!($buf, $offset) as u64,
            8 => read_int_from_buf_u64!($buf, $offset),
            _ => panic!("Invalid size"),
        };

        $offset += $size as usize;
        value
    }};
}

macro_rules! read_int_from_buf_le {
    ($buf:expr, $offset:expr, $size:expr) => {{
        let value = match $size {
            1 => read_int_from_buf_le_u8!($buf, $offset) as u64,
            2 => read_int_from_buf_le_u16!($buf, $offset) as u64,
            4 => read_int_from_buf_le_u32!($buf, $offset) as u64,
            8 => read_int_from_buf_le_u64!($buf, $offset),
            _ => panic!("Invalid size"),
        };

        $offset += $size as usize;
        value
    }};
}

pub(crate) use read_int_from_buf;
pub(crate) use read_int_from_buf_le;
