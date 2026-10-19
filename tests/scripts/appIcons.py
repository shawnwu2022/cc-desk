"""Independently verify approved app icon pixels with Python 3 + Pillow."""
import hashlib
import json
import struct
from pathlib import Path
from xml.etree import ElementTree

from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
SOURCE = ROOT / 'src-tauri/icons/source'
SIZES = (16, 24, 32, 48, 64, 128, 256, 512, 1024)
ICO_SIZES = SIZES[:7]


# 逐个检查交付文件，避免换行转换或替换文件后误用旧校验说明。
def AppIcon_SourceManifest_001():
    rows = (SOURCE / 'SHA256SUMS.txt').read_text(encoding='utf-8').splitlines()
    paths = set()
    for row in rows:
        digest, relative = row.split('  ', 1)
        assert relative not in paths and '..' not in Path(relative).parts
        paths.add(relative)
        assert hashlib.sha256((SOURCE / relative).read_bytes()).hexdigest() == digest, relative
    assert len(paths) == 19
    assert paths == {p.relative_to(SOURCE).as_posix() for p in SOURCE.rglob('*') if p.is_file() and p.name != 'SHA256SUMS.txt'}
    for path in (SOURCE / 'svg').glob('*.svg'):
        svg = ElementTree.parse(path)
        for node in svg.iter():
            assert node.tag.split('}')[-1] in {'svg', 'title', 'desc', 'rect', 'path'}, path
            if 'fill' in node.attrib:
                assert node.attrib['fill'] in {'#35404B', '#FA8939', '#FFFFFF'}, path


# 透明度须覆盖完整空白边界，不能只以 RGBA 通道的存在冒充透明。
def AppIcon_PngTransparency_002():
    images = {}
    for size in SIZES:
        with Image.open(SOURCE / f'png/ccdesk-{size}.png') as original:
            assert original.mode == 'RGBA' and original.size == (size, size)
            image = original.copy()
        alpha = image.getchannel('A')
        assert alpha.getextrema() == (0, 255), size
        x0, y0, x1, y1 = alpha.getbbox()
        assert 0 < x0 < x1 < size and 0 < y0 < y1 < size, size
        images[size] = image
    return images


# ICO 每档像素与原 PNG 完全一致，同时验证小档 DIB 和 256 PNG 的封装。
def AppIcon_IcoFrames_003(images):
    data = (ROOT / 'src-tauri/icons/icon.ico').read_bytes()
    assert struct.unpack_from('<HHH', data) == (0, 1, 7)
    end = 6 + 16 * 7
    for index, size in enumerate(ICO_SIZES):
        width, height, colors, reserved, planes, bits, length, offset = struct.unpack_from('<BBBBHHII', data, 6 + index * 16)
        assert (width or 256, height or 256, colors, reserved, planes, bits) == (size, size, 0, 0, 1, 32)
        assert offset == end and offset + length <= len(data)
        frame = data[offset:offset + length]
        if size == 256:
            assert frame == (SOURCE / 'png/ccdesk-256.png').read_bytes()
        else:
            assert struct.unpack_from('<IiiHH', frame) == (40, size, size * 2, 1, 32)
            assert length == 40 + size * size * 4 + ((size + 31) // 32) * 4 * size
        end = offset + length
    assert end == len(data)
    with Image.open(ROOT / 'src-tauri/icons/icon.ico') as icon:
        assert icon.ico.sizes() == {(size, size) for size in ICO_SIZES}
        for size in ICO_SIZES:
            assert icon.ico.getimage((size, size)).convert('RGBA').tobytes() == images[size].tobytes(), size


# 用独立解码器核对每种标准/Retina 表示，包括 Tauri 编码的旧式 RGB+mask。
def AppIcon_IcnsPixels_004(images):
    data = (ROOT / 'src-tauri/icons/icon.icns').read_bytes()
    assert data[:4] == b'icns' and struct.unpack_from('>I', data, 4)[0] == len(data)
    with Image.open(ROOT / 'src-tauri/icons/icon.icns') as icon:
        expected = {(16, 16, 1), (32, 32, 1), (16, 16, 2), (32, 32, 2), (128, 128, 1),
                    (128, 128, 2), (256, 256, 1), (256, 256, 2), (512, 512, 1), (512, 512, 2)}
        assert set(icon.info['sizes']) == expected, icon.info['sizes']
        for size in sorted(expected):
            actual = icon.icns.getimage(size).convert('RGBA')
            physical = size[0] * size[2]
            assert actual.size == (physical, physical)
            assert actual.tobytes() == images[physical].tobytes(), size


# 生产引用继续使用正式生成件，第三方 CLI 标识由各自资源承载。
def AppIcon_ConsumerCopies_005():
    copies = {'src/assets/icons/app-icon.png': 'png/ccdesk-256.png',
              'src/assets/icons/app-icon.ico': 'windows/ccdesk.ico',
              'src-tauri/icons/icon.ico': 'windows/ccdesk.ico',
              'src-tauri/icons/32x32.png': 'png/ccdesk-32.png',
              'src-tauri/icons/128x128.png': 'png/ccdesk-128.png',
              'src-tauri/icons/128x128@2x.png': 'png/ccdesk-256.png',
              'src-tauri/icons/cc-desk-icon-source.png': 'png/ccdesk-1024.png'}
    copies.update({f'src/assets/icons/app-icon-{size}.png': f'png/ccdesk-{size}.png' for size in SIZES[:5]})
    for output, original in copies.items():
        assert (ROOT / output).read_bytes() == (SOURCE / original).read_bytes(), output
    config = json.loads((ROOT / 'src-tauri/tauri.conf.json').read_text(encoding='utf-8'))
    assert config['bundle']['icon'] == ['icons/32x32.png', 'icons/128x128.png', 'icons/128x128@2x.png', 'icons/icon.icns', 'icons/icon.ico']
    assert config['bundle']['windows']['nsis']['installerIcon'] == 'icons/icon.ico'
    assert 'vite.svg' not in (ROOT / 'index.html').read_text(encoding='utf-8')


if __name__ == '__main__':
    AppIcon_SourceManifest_001()
    approved = AppIcon_PngTransparency_002()
    AppIcon_IcoFrames_003(approved)
    AppIcon_IcnsPixels_004(approved)
    AppIcon_ConsumerCopies_005()
    print('PASS: 19 source hashes; 9 RGBA PNGs; 7 exact ICO frames; 10 exact ICNS representations; frontend/native copies and packaging paths.')
