"""Synthetic content tags, written by the pinned reference's Exiv2 binding."""
from pathlib import Path
from PIL import Image
import exiv2
path=Path(__file__).resolve().parents[1]/'tests/fixtures/core/metadata.png'
Image.new('RGB',(4,3),(32,64,128)).save(path)
m=exiv2.ImageFactory.open(str(path));m.readMetadata()
m.exifData()['Exif.Image.Artist']='Metadata fixture'
m.exifData()['Exif.Image.Orientation']=6
m.exifData()['Exif.Photo.PixelXDimension']=4
m.exifData()['Exif.Photo.PixelYDimension']=3
m.iptcData()['Iptc.Application2.Caption']='Synthetic caption'
m.xmpData()['Xmp.dc.description']='Synthetic description'
m.writeMetadata()
