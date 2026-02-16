#[macro_export]
macro_rules! versioned_read_try {
    // Dispatch a BinRead::read between two versioned raw types and run
    // the provided block with the parsed `raw` bound.
    ($ver:expr, $reader:expr,
     v1: $V1:ty,
     v2: $V2:ty,
     as $raw:ident => $block:block
    ) => {{
        if $ver.major() == 1 {
            let $raw: $V1 = <$V1>::read($reader)?;
            $block
        } else {
            let $raw: $V2 = <$V2>::read($reader)?;
            $block
        }
    }};
}

#[macro_export]
macro_rules! versioned_read_unwrap {
    // Like `versioned_read_try!` but unwraps the read (panics on error).
    ($ver:expr, $reader:expr,
     v1: $V1:ty,
     v2: $V2:ty,
     as $raw:ident => $block:block
    ) => {{
        if $ver.major() == 1 {
            let $raw: $V1 = <$V1>::read($reader).unwrap();
            $block
        } else {
            let $raw: $V2 = <$V2>::read($reader).unwrap();
            $block
        }
    }};
}
