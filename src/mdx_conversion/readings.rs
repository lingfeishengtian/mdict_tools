use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::BufWriter;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::io::Write;
use std::path::Path;

use binrw::{BinRead, BinWrite};

use crate::error::{MDictError, Result};

const READINGS_ENTRY_HEADER_SIZE: u64 = 12;

#[derive(Debug, Clone, BinRead, BinWrite)]
#[brw(little)]
pub struct ReadingsEntryHeader {
    pub length: u32,
    pub link_id: u64,
}

#[derive(Debug, Clone)]
pub struct ReadingsEntry {
    pub length: u32,
    pub link_id: u64,
    pub readings: Vec<String>,
    pub entry_size: u64,
}

fn parse_readings_payload(payload: &[u8]) -> Result<Vec<String>> {
    let mut readings = Vec::new();
    let mut start = 0usize;

    for (idx, &byte) in payload.iter().enumerate() {
        if byte != 0 {
            continue;
        }

        if idx > start {
            let segment = &payload[start..idx];
            let reading = std::str::from_utf8(segment)
                .map(str::to_owned)
                .map_err(|e| MDictError::InvalidFormat(format!("invalid utf8 reading: {}", e)))?;
            readings.push(reading);
        }
        start = idx + 1;
    }

    if start < payload.len() {
        let segment = &payload[start..];
        let reading = std::str::from_utf8(segment)
            .map(str::to_owned)
            .map_err(|e| MDictError::InvalidFormat(format!("invalid utf8 reading: {}", e)))?;
        readings.push(reading);
    }

    Ok(readings)
}

fn serialize_readings_entry(remapped_link: u64, readings: &HashSet<String>) -> Result<Vec<u8>> {
    let mut sorted_readings: Vec<&str> = readings.iter().map(String::as_str).collect();
    sorted_readings.sort_unstable();
    let payload_len: usize = sorted_readings.iter().map(|reading| reading.len()).sum::<usize>()
        + sorted_readings.len().saturating_sub(1);

    let header = ReadingsEntryHeader {
        length: payload_len as u32,
        link_id: remapped_link,
    };

    let mut out = Vec::with_capacity(READINGS_ENTRY_HEADER_SIZE as usize + payload_len);
    let mut cursor = Cursor::new(&mut out);
    header.write_le(&mut cursor)?;

    for (idx, reading) in sorted_readings.iter().enumerate() {
        if idx > 0 {
            out.push(0);
        }
        out.extend_from_slice(reading.as_bytes());
    }

    Ok(out)
}

pub fn write_readings_data_and_collect_key_offsets(
    readings_list: &HashMap<u64, HashSet<String>>,
    link_order: &[u64],
    link_remap: &HashMap<u64, u64>,
    readings_path: impl AsRef<Path>,
) -> Result<HashMap<String, u64>> {
    let estimated_keys = readings_list.values().map(HashSet::len).sum();
    let mut key_link_map = HashMap::with_capacity(estimated_keys);
    let output_file = File::create(readings_path)?;
    let mut writer = BufWriter::new(output_file);
    let mut current_offset = 0u64;

    for &old_link in link_order {
        let Some(indices) = readings_list.get(&old_link) else {
            continue;
        };

        let remapped_link = *link_remap.get(&old_link).ok_or_else(|| {
            MDictError::InvalidArgument(format!("missing remapped link for old link {}", old_link))
        })?;

        let entry_bytes = serialize_readings_entry(remapped_link, indices)?;
        let entry_len = entry_bytes.len() as u64;
        writer.write_all(&entry_bytes)?;

        for index in indices {
            key_link_map
                .entry(index.clone())
                .or_insert(current_offset);
        }

        current_offset = current_offset
            .checked_add(entry_len)
            .ok_or_else(|| MDictError::InvalidFormat("readings file offset overflow".to_string()))?;
    }

    writer.flush()?;

    Ok(key_link_map)
}

pub fn read_entry_from_offset<R: Read + Seek>(
    reader: &mut R,
    offset: u64,
) -> Result<ReadingsEntry> {
    reader.seek(SeekFrom::Start(offset))?;
    let header = ReadingsEntryHeader::read_le(reader)?;

    let payload_len = usize::try_from(header.length)
        .map_err(|_| MDictError::InvalidFormat("readings payload length overflow".to_string()))?;
    let mut payload = vec![0u8; payload_len];
    reader.read_exact(&mut payload)?;
    let readings = parse_readings_payload(&payload)?;

    Ok(ReadingsEntry {
        length: header.length,
        link_id: header.link_id,
        readings,
        entry_size: READINGS_ENTRY_HEADER_SIZE + header.length as u64,
    })
}

pub fn read_header_from_bytes_result(bytes: &[u8], offset: u64) -> Result<ReadingsEntryHeader> {
    let start = usize::try_from(offset)
        .map_err(|_| MDictError::InvalidFormat("readings header offset overflow".to_string()))?;
    let end = start
        .checked_add(READINGS_ENTRY_HEADER_SIZE as usize)
        .ok_or_else(|| MDictError::InvalidFormat("readings header range overflow".to_string()))?;
    let slice = bytes.get(start..end).ok_or_else(|| {
        MDictError::InvalidFormat(format!(
            "readings header out of bounds at offset {}",
            offset
        ))
    })?;

    let mut cursor = Cursor::new(slice);
    Ok(ReadingsEntryHeader::read_le(&mut cursor)?)
}

pub fn read_header_from_bytes(bytes: &[u8], offset: u64) -> Option<ReadingsEntryHeader> {
    read_header_from_bytes_result(bytes, offset).ok()
}

pub fn read_entry_from_bytes_result(bytes: &[u8], offset: u64) -> Result<ReadingsEntry> {
    let header = read_header_from_bytes_result(bytes, offset)?;
    let start = usize::try_from(offset)
        .map_err(|_| MDictError::InvalidFormat("readings entry offset overflow".to_string()))?;
    let payload_start = start
        .checked_add(READINGS_ENTRY_HEADER_SIZE as usize)
        .ok_or_else(|| MDictError::InvalidFormat("readings payload start overflow".to_string()))?;
    let payload_len = usize::try_from(header.length)
        .map_err(|_| MDictError::InvalidFormat("readings payload length overflow".to_string()))?;
    let payload_end = payload_start
        .checked_add(payload_len)
        .ok_or_else(|| MDictError::InvalidFormat("readings payload end overflow".to_string()))?;

    let payload = bytes.get(payload_start..payload_end).ok_or_else(|| {
        MDictError::InvalidFormat(format!(
            "readings payload out of bounds at offset {}",
            offset
        ))
    })?;

    let readings = parse_readings_payload(payload)?;

    Ok(ReadingsEntry {
        length: header.length,
        link_id: header.link_id,
        readings,
        entry_size: READINGS_ENTRY_HEADER_SIZE + header.length as u64,
    })
}

pub fn read_entry_from_bytes(bytes: &[u8], offset: u64) -> Option<ReadingsEntry> {
    read_entry_from_bytes_result(bytes, offset).ok()
}

pub fn read_link_id_from_offset<R: Read + Seek>(
    reader: &mut R,
    offset: u64,
) -> Result<Option<u64>> {
    let file_len = reader.seek(SeekFrom::End(0))?;
    if offset
        .checked_add(READINGS_ENTRY_HEADER_SIZE)
        .is_none_or(|end| end > file_len)
    {
        return Ok(None);
    };

    reader.seek(SeekFrom::Start(offset))?;
    let header = ReadingsEntryHeader::read_le(reader)?;
    Ok(Some(header.link_id))
}
