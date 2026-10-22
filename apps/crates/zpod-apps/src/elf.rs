//! Minimal ELF inspection so `zpod-app install` can refuse binaries that
//! would die with SIGILL on the Pi Zero's ARM1176 (ARMv6) core, e.g. a
//! binary built for armv7-unknown-linux-gnueabihf by mistake.

#[derive(Debug, PartialEq, Eq)]
pub enum Check {
    /// Not an ELF file (a script, data): nothing to check.
    NotElf,
    /// 32-bit little-endian ARM, CPU architecture compatible with ARMv6.
    Ok { arch: Option<u64> },
    Incompatible(String),
}

/// Names for Tag_CPU_arch values (ARM IHI 0045).
pub fn arch_name(v: u64) -> &'static str {
    match v {
        0 => "Pre-v4",
        1 => "v4",
        2 => "v4T",
        3 => "v5T",
        4 => "v5TE",
        5 => "v5TEJ",
        6 => "v6",
        7 => "v6KZ",
        8 => "v6T2",
        9 => "v6K",
        10 => "v7",
        11 => "v6-M",
        12 => "v6S-M",
        13 => "v7E-M",
        14 => "v8",
        _ => "unknown",
    }
}

/// ARM1176JZF-S implements v6KZ (which includes v6K); not v6T2 (Thumb-2) or later.
fn armv6_compatible(v: u64) -> bool {
    v <= 7 || v == 9
}

fn u16le(b: &[u8], o: usize) -> Option<u64> {
    Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?) as u64)
}
fn u32le(b: &[u8], o: usize) -> Option<u64> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?) as u64)
}

fn uleb(b: &[u8], pos: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    let mut shift = 0;
    loop {
        let c = *b.get(*pos)?;
        *pos += 1;
        v |= ((c & 0x7f) as u64) << shift;
        if c & 0x80 == 0 {
            return Some(v);
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }
}

fn ntbs(b: &[u8], pos: &mut usize) -> Option<()> {
    let n = b.get(*pos..)?.iter().position(|&c| c == 0)?;
    *pos += n + 1;
    Some(())
}

/// Parse an `.ARM.attributes` section and return Tag_CPU_arch.
pub fn parse_cpu_arch(sec: &[u8]) -> Option<u64> {
    if sec.first() != Some(&b'A') {
        return None;
    }
    let mut p = 1;
    while p + 4 <= sec.len() {
        let len = u32le(sec, p)? as usize;
        let end = p.checked_add(len)?.min(sec.len());
        let mut q = p + 4;
        let vendor_start = q;
        ntbs(sec, &mut q)?;
        let vendor = &sec[vendor_start..q - 1];
        if vendor == b"aeabi" {
            while q + 5 <= end {
                let tag = sec[q];
                let sublen = u32le(sec, q + 1)? as usize;
                let subend = q.checked_add(sublen)?.min(end);
                let mut r = q + 5;
                if tag == 1 {
                    // File attributes
                    while r < subend {
                        let t = uleb(sec, &mut r)?;
                        match t {
                            6 => return uleb(sec, &mut r),
                            4 | 5 | 67 => ntbs(sec, &mut r)?,
                            32 => {
                                uleb(sec, &mut r)?;
                                ntbs(sec, &mut r)?;
                            }
                            t if t >= 32 && t % 2 == 1 => ntbs(sec, &mut r)?,
                            _ => {
                                uleb(sec, &mut r)?;
                            }
                        }
                    }
                }
                if sublen == 0 {
                    break;
                }
                q = subend;
            }
        }
        if len == 0 {
            break;
        }
        p = end;
    }
    None
}

pub fn check(bytes: &[u8]) -> Check {
    if bytes.len() < 52 || &bytes[0..4] != b"\x7fELF" {
        return Check::NotElf;
    }
    if bytes[4] != 1 {
        return Check::Incompatible("64-bit ELF (the Pi Zero W is 32-bit ARMv6)".into());
    }
    if bytes[5] != 1 {
        return Check::Incompatible("big-endian ELF".into());
    }
    let machine = u16le(bytes, 18).unwrap_or(0);
    if machine != 40 {
        let what = match machine {
            3 => "x86",
            62 => "x86-64",
            183 => "AArch64",
            243 => "RISC-V",
            94 => "Xtensa",
            _ => "non-ARM",
        };
        return Check::Incompatible(format!("{what} binary (e_machine {machine}), need 32-bit ARM"));
    }
    let arch = (|| {
        let shoff = u32le(bytes, 32)? as usize;
        let shentsize = u16le(bytes, 46)? as usize;
        let shnum = u16le(bytes, 48)? as usize;
        let shstrndx = u16le(bytes, 50)? as usize;
        let sh = |i: usize| shoff.checked_add(i.checked_mul(shentsize)?);
        let strtab = sh(shstrndx)?;
        let str_off = u32le(bytes, strtab + 16)? as usize;
        for i in 0..shnum {
            let h = sh(i)?;
            let name = u32le(bytes, h)? as usize;
            let ty = u32le(bytes, h + 4)?;
            if ty != 0x7000_0003 {
                continue; // SHT_ARM_ATTRIBUTES
            }
            let nm = bytes.get(str_off + name..)?;
            let nm = &nm[..nm.iter().position(|&c| c == 0)?];
            if nm != b".ARM.attributes" {
                continue;
            }
            let off = u32le(bytes, h + 16)? as usize;
            let size = u32le(bytes, h + 20)? as usize;
            return parse_cpu_arch(bytes.get(off..off.checked_add(size)?)?);
        }
        None
    })();
    match arch {
        Some(v) if !armv6_compatible(v) => Check::Incompatible(format!(
            "built for ARM {} (Tag_CPU_arch {v}); the Pi Zero W needs v6 — use target arm-unknown-linux-musleabihf/gnueabihf",
            arch_name(v)
        )),
        a => Check::Ok { arch: a },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attrs(arch: u8) -> Vec<u8> {
        // 'A' | len | "aeabi\0" | tag 1 | len | Tag_CPU_name "6KZ\0" | Tag_CPU_arch arch | Tag_ARM_ISA_use 1
        let mut file = vec![5u8];
        file.extend_from_slice(b"6KZ\0");
        file.extend_from_slice(&[6, arch, 8, 1]);
        let mut sub = vec![1u8];
        sub.extend_from_slice(&((5 + file.len()) as u32).to_le_bytes());
        sub.extend_from_slice(&file);
        let mut vend = b"aeabi\0".to_vec();
        vend.extend_from_slice(&sub);
        let mut out = vec![b'A'];
        out.extend_from_slice(&((4 + vend.len()) as u32).to_le_bytes());
        out.extend_from_slice(&vend);
        out
    }

    #[test]
    fn attributes_parse() {
        assert_eq!(parse_cpu_arch(&attrs(6)), Some(6));
        assert_eq!(parse_cpu_arch(&attrs(10)), Some(10));
        assert_eq!(parse_cpu_arch(b"B"), None);
    }

    #[test]
    fn non_elf_and_foreign() {
        assert_eq!(check(b"#!/bin/sh\necho hi\n"), Check::NotElf);
        let mut x86 = vec![0u8; 64];
        x86[..4].copy_from_slice(b"\x7fELF");
        x86[4] = 2;
        x86[5] = 1;
        x86[18] = 62;
        assert!(matches!(check(&x86), Check::Incompatible(_)));
    }

    /// The binary running this test: on the ARM cross target it must be OK
    /// with v6; on the host it must be rejected as non-ARM.
    #[test]
    fn self_binary() {
        let me = std::fs::read(std::env::current_exe().unwrap()).unwrap();
        let r = check(&me);
        if cfg!(target_arch = "arm") {
            assert!(matches!(r, Check::Ok { arch: Some(6) | Some(7) }), "{r:?}");
        } else {
            assert!(matches!(r, Check::Incompatible(_)), "{r:?}");
        }
    }
}
