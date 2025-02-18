# Cross Compiled Mdict Reader Library

MDX and MDD file reader library for Android, iOS, macOS, Linux, Windows, and WebAssembly.

## Features being developed

- [x] Support MDX and MDD file format.
- [x] Support Android, iOS, macOS, Linux, Windows, and WebAssembly.
- [x] Support searching. (Algorithms uses binary search and prefix sums while attempting to optimize IO operations)

## RAM Usage and Performance

- [x] The library is designed to be memory efficient, however, can use up to 10 MB of RAM for large dictionaries.

## Usage

Will be implemented into [CJE Dictionary](https://github.com/lingfeishengtian/CJE-Dictionary)

## Testing

Used jitendex to test. Many tests search for a word in the Japanese dictionary.

## Credits

- This project is inspired from [writemdict](https://github.com/zhansliu/writemdict/tree/master) saving me a lot of time reverse engineering the MDX and MDD file format.