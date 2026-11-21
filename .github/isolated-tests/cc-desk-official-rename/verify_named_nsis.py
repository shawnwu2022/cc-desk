"""Read-only narrow verifier for the known Tauri NSIS 3.11 Unicode solid-LZMA profile.

Uses published NSIS header/entry semantics, not filename string scanning or guessing
by PE size/marker. No interpreter, installer execution, extraction or loose output.
The required SHA-256 must come from an independent immutable receipt.
"""
import argparse
import hashlib
import hmac
import json
import lzma
import os
import re
import stat
import struct
import zlib

MAX_INSTALLER = 50 * 1024 * 1024
MAX_STREAM = 150 * 1024 * 1024
MAX_HEADER = 512 * 1024
MAGIC = b"\xef\xbe\xad\xdeNullsoftInst"
MEMBER = "cc-desk.exe"
ENTRY_SIZE = 28
SECTION_SIZE = 2072  # 6 int32 fields followed by NSIS Unicode name[1024].
EXTRACT_FILE = 20
CREATE_DIR = 11
INSTDIR = "\x03\u8095"  # NS_VAR_CODE + encoded variable slot 21.
PLUGINSDIR = "\x03\u809a"  # NS_VAR_CODE + encoded variable slot 26.
STATIC_NAME = re.compile(r"[A-Za-z0-9._-]+(?:\\[A-Za-z0-9._-]+)*\Z")
DIGEST = re.compile(r"[a-f0-9]{64}\Z")


class Rejected(ValueError):
    pass


def require(value, code):
    if not value:
        raise Rejected(code)


def u32(data, at):
    require(0 <= at <= len(data) - 4, "BOUNDS")
    return struct.unpack_from("<I", data, at)[0]


def pe_machine(data):
    require(len(data) >= 96 and bytes(data[:2]) == b"MZ", "PE")
    at = u32(data, 0x3c)
    require(0x40 <= at <= len(data) - 26 and bytes(data[at:at + 4]) == b"PE\0\0", "PE")
    machine = struct.unpack_from("<H", data, at + 4)[0]
    optional = struct.unpack_from("<H", data, at + 24)[0]
    require((machine, optional) in {(0x14c, 0x10b), (0x8664, 0x20b)}, "PE_MACHINE")
    return machine


def decode(archive, expected):
    require(isinstance(expected, str) and DIGEST.fullmatch(expected), "DIGEST_FORMAT")
    require(1024 <= len(archive) <= MAX_INSTALLER, "ARCHIVE_SIZE")
    actual = hashlib.sha256(archive).hexdigest()
    require(hmac.compare_digest(actual, expected), "ARCHIVE_DIGEST")
    require(pe_machine(archive) == 0x14c, "PROFILE_STUB")
    magic = archive.find(MAGIC)
    at = magic - 4
    require(magic >= 516 and at % 512 == 0 and archive.find(MAGIC, magic + 1) == -1, "NSIS_HEADER")
    require(u32(archive, at) == 0, "PROFILE_FLAGS")
    header_size, full_size = u32(archive, at + 20), u32(archive, at + 24)
    require(300 <= header_size <= MAX_HEADER and at + full_size == len(archive), "NSIS_SIZE")
    require(zlib.crc32(memoryview(archive)[512:-4]) == u32(archive, len(archive) - 4), "NSIS_CRC")
    compressed = at + 28
    require(archive[compressed:compressed + 5] == b"\x5d\x00\x00\x80\x00", "PROFILE_LZMA")
    decoder = lzma.LZMADecompressor(format=lzma.FORMAT_RAW, filters=[
        {"id": lzma.FILTER_LZMA1, "dict_size": 8 * 1024 * 1024, "lc": 3, "lp": 0, "pb": 2}
    ])
    try:
        decoded = decoder.decompress(archive[compressed + 5:-4], max_length=MAX_STREAM)
    except lzma.LZMAError:
        raise Rejected("NSIS_LZMA") from None
    require(decoder.eof and not decoder.unused_data and len(decoded) < MAX_STREAM, "NSIS_STREAM_BOUNDS")
    require(u32(decoded, 0) == header_size and 4 + header_size < len(decoded), "NSIS_HEADER_FRAME")
    return decoded, header_size


def named_frame(decoded, header_size):
    """Map the literal install instruction to its exact data frame; return only a memoryview."""
    require(300 <= header_size <= MAX_HEADER and u32(decoded, 0) == header_size and
            4 + header_size < len(decoded), "NSIS_HEADER_FRAME")
    header = memoryview(decoded)[4:4 + header_size]
    blocks = [struct.unpack_from("<II", header, 4 + 8 * i) for i in range(8)]
    (pages, pn), (sections, sn), (entries, en), (strings, stn), (languages, ln), (colors, _), bg, data = blocks
    require(72 <= pages < sections < entries < strings < languages <= colors <= header_size and
            1 <= pn <= 64 and 1 <= sn <= 32 and 1 <= en <= 10000 and
            sections + sn * SECTION_SIZE == entries and entries + en * ENTRY_SIZE == strings and
            stn == 0 and ln >= 1 and bg == data == (0, 0) and
            strings % 2 == languages % 2 == 0, "PROFILE_BLOCKS")
    st = bytes(header[strings:languages])

    def str_at(index):
        at = index * 2
        # NSIS interns common suffixes; a valid operand may start after the
        # opening bytes of another string. Its UTF-16 offset remains authoritative.
        require(0 <= at < len(st) - 1, "STRING_START")
        end = next((i for i in range(at, min(at + 2048, len(st)), 2) if st[i:i + 2] == b"\0\0"), None)
        require(end is not None, "STRING_BOUND")
        try:
            return st[at:end].decode("utf-16-le", errors="strict")
        except UnicodeDecodeError:
            raise Rejected("STRING_UTF16") from None

    rows = [struct.unpack_from("<7i", header, entries + i * ENTRY_SIZE) for i in range(en)]
    ranges = []
    for i in range(sn):
        label, _, flags, start, count, _ = struct.unpack_from("<6i", header, sections + i * SECTION_SIZE)
        require(0 <= start < en and 0 <= count <= en - start, "SECTION_RANGE")
        data_name = str_at(label)
        ranges.append((data_name, flags, start, start + count))
    install = [r for r in ranges if r[0].casefold() == "install"]
    require(len(install) == 1 and install[0][0] == "Install", "INSTALL_SECTION")
    _, flags, start, end = install[0]
    require(flags == 1 and start < end, "INSTALL_SECTION")
    # NSIS compiler uses SetOutPath $INSTDIR as the first entry in Tauri's Install section.
    require(rows[start][0] == CREATE_DIR and rows[start][2:] == (1, 0, 0, 0, 0)
            and str_at(rows[start][1]) == INSTDIR, "INSTALL_OUTPUT_CONTEXT")

    data_start = 4 + header_size
    frames = {}
    pos = data_start
    while pos < len(decoded):
        rel = pos - data_start
        n = u32(decoded, pos)
        require(n > 0 and n <= len(decoded) - pos - 4 and len(frames) < 256, "FRAME_BOUNDS")
        frames[rel] = (pos + 4, pos + 4 + n)
        pos += 4 + n
    require(pos == len(decoded), "FRAME_BOUNDS")

    # Accept only literal relative names or the known NSIS plugins-directory prefix.
    # Do not evaluate variables, arbitrary Windows paths, aliases or installer branches.
    named = []
    extracts = []
    for i, r in enumerate(rows):
        if r[0] != EXTRACT_FILE:
            continue
        name = str_at(r[2])
        expected_context = name.startswith(PLUGINSDIR + "\\")
        relative = name[len(PLUGINSDIR) + 1:] if expected_context else name
        require(STATIC_NAME.fullmatch(relative) and
                all(piece not in {".", ".."} and not piece.endswith(".") for piece in relative.split("\\")),
                "EXTRACT_NAME")
        require(r[3] in frames, "EXTRACT_OFFSET")
        extracts.append((i, r[3]))
        if relative.rsplit("\\", 1)[-1].casefold() == MEMBER:
            named.append((i, name, r[3]))
    require(len(named) == 1 and named[0][1] == MEMBER, "MEMBER_AMBIGUOUS")
    i, name, offset = named[0]
    require(start < i < end and sum(s <= i < e for _, _, s, e in ranges) == 1 and
            not any(row[0] == CREATE_DIR and row[2] != 0 for row in rows[start+1:i]),
            "MEMBER_CONTEXT")
    require(sum(off == offset for _, off in extracts) == 1, "MEMBER_OFFSET_AMBIGUOUS")
    a, b = frames[offset]
    exe = memoryview(decoded)[a:b]
    require(pe_machine(exe) == 0x8664, "MEMBER_MACHINE")
    return exe


def verify(archive, expected):
    decoded, hs = decode(archive, expected)
    member = named_frame(decoded, hs)
    return {"installerSha256": expected, "member": MEMBER, "memberSha256": hashlib.sha256(member).hexdigest(),
            "memberBytes": len(member), "format": "tauri-nsis-unicode-solid-lzma-x64"}


def from_file(path, expected):
    stat0 = os.lstat(path)
    require(stat.S_ISREG(stat0.st_mode) and stat0.st_size <= MAX_INSTALLER and
            not getattr(stat0, "st_file_attributes", 0) & 0x400, "INPUT_FILE")
    flags = os.O_RDONLY | getattr(os, "O_BINARY", 0) | getattr(os, "O_NONBLOCK", 0) | getattr(os, "O_NOFOLLOW", 0)
    fd = os.open(path, flags)
    with os.fdopen(fd, "rb") as handle:
        stat1 = os.fstat(handle.fileno())
        require(stat.S_ISREG(stat1.st_mode) and os.path.samestat(stat0, stat1) and
                not getattr(stat1, "st_file_attributes", 0) & 0x400, "INPUT_REPLACED")
        data = handle.read(MAX_INSTALLER + 1)
    require(len(data) == stat0.st_size, "INPUT_SIZE")
    return verify(data, expected)


if __name__ == "__main__":
    cli = argparse.ArgumentParser(description="Hash one named CC Desk payload without extracting or running it.")
    cli.add_argument("--installer", required=True)
    cli.add_argument("--sha256", required=True)
    args = cli.parse_args()
    try:
        print(json.dumps(from_file(args.installer, args.sha256), sort_keys=True))
    except Rejected as exc:
        print(json.dumps({"rejected": str(exc)}))
        raise SystemExit(1)
    except OSError:
        print(json.dumps({"rejected": "INPUT_IO"}))
        raise SystemExit(1)
