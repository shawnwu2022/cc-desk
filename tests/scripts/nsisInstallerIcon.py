"""Compare the installer PE icon resources with the approved ICO, without running it."""
import hashlib
import struct
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def resources(data):
    pe = struct.unpack_from('<I', data, 0x3c)[0]
    assert data[pe:pe + 4] == b'PE\0\0'
    count = struct.unpack_from('<H', data, pe + 6)[0]
    optional_size = struct.unpack_from('<H', data, pe + 20)[0]
    optional = pe + 24
    magic = struct.unpack_from('<H', data, optional)[0]
    assert magic in (0x10b, 0x20b)
    directory = optional + (112 if magic == 0x20b else 96)
    resource_rva = struct.unpack_from('<I', data, directory + 16)[0]
    sections = []
    for index in range(count):
        offset = optional + optional_size + 40 * index
        virtual_size, rva, raw_size, raw = struct.unpack_from('<IIII', data, offset + 8)
        sections.append((rva, max(virtual_size, raw_size), raw))

    def file_offset(rva):
        for start, length, raw in sections:
            if start <= rva < start + length:
                return raw + rva - start
        raise AssertionError(f'Unmapped resource RVA: {rva}')

    base = file_offset(resource_rva)
    found = {}

    def walk(relative, path):
        offset = base + relative
        named, numbered = struct.unpack_from('<HH', data, offset + 12)
        for index in range(named + numbered):
            name, child = struct.unpack_from('<II', data, offset + 16 + 8 * index)
            key = path + (name,)
            if child & 0x80000000:
                walk(child & 0x7fffffff, key)
            else:
                rva, size = struct.unpack_from('<II', data, base + child)
                start = file_offset(rva)
                found[key] = data[start:start + size]
    walk(0, ())
    return found


# 解析安装器外壳的 RT_GROUP_ICON/RT_ICON，不将应用内嵌文件或截图当作外壳图标证据。
def InstallerIcon_ApprovedFrames_001(installer):
    data = installer.read_bytes()
    ico = (ROOT / 'src-tauri/icons/icon.ico').read_bytes()
    approved = {}
    for index in range(struct.unpack_from('<H', ico, 4)[0]):
        offset = 6 + 16 * index
        size, start = struct.unpack_from('<II', ico, offset + 8)
        approved[ico[offset:offset + 8]] = ico[start:start + size]
    found = resources(data)
    groups = [(key, value) for key, value in found.items() if key[0] == 14]
    assert groups, 'Installer has no RT_GROUP_ICON'
    matched = 0
    for key, group in groups:
        actual = {}
        for index in range(struct.unpack_from('<H', group, 4)[0]):
            offset = 6 + 14 * index
            size, resource_id = struct.unpack_from('<IH', group, offset + 8)
            frame = found[(3, resource_id, key[2])]
            assert len(frame) == size
            actual[group[offset:offset + 8]] = frame
        if actual == approved:
            matched += 1
    assert matched, 'No installer icon group matches all approved ICO frames byte-for-byte'
    print(f'PASS: {installer}; {matched} approved icon group(s), {len(approved)} exact frames; SHA256 {hashlib.sha256(data).hexdigest()}')


if __name__ == '__main__':
    assert len(sys.argv) == 2, 'Usage: python tests/scripts/nsisInstallerIcon.py INSTALLER.exe'
    InstallerIcon_ApprovedFrames_001(Path(sys.argv[1]))
