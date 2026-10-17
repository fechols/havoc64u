"""Convert the host tool's RGB PPM previews to PNG without third-party modules."""
from pathlib import Path
import struct
import sys
import zlib

def png(width, height, pixels):
    def chunk(kind, data):
        return struct.pack('>I', len(data))+kind+data+struct.pack('>I',zlib.crc32(kind+data))
    rows=b''.join(b'\0'+pixels[y*width*3:(y+1)*width*3] for y in range(height))
    return b'\x89PNG\r\n\x1a\n'+chunk(b'IHDR',struct.pack('>IIBBBBB',width,height,8,2,0,0,0))+chunk(b'IDAT',zlib.compress(rows))+chunk(b'IEND',b'')

directory=Path(sys.argv[1]) if len(sys.argv)>1 else Path(__file__).resolve().parent.parent/'target/previews'
for path in sorted(directory.glob('*.ppm')):
    magic,dimensions,maximum,pixels=path.read_bytes().split(b'\n',3)
    assert magic==b'P6' and maximum==b'255'
    width,height=map(int,dimensions.split());assert len(pixels)==width*height*3
    output=path.with_suffix('.png');output.write_bytes(png(width,height,pixels));print(output)
