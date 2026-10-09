"""Verify terrorbats.exe embeds the canonical icon: enumerate RT_GROUP_ICON
resources via pefile and compare the largest extracted image against the
committed ICO entry bytes."""
import struct
import sys
import pefile

exe_path, ico_path = sys.argv[1], sys.argv[2]
pe = pefile.PE(exe_path)

RT_GROUP_ICON = 14
RT_ICON = 3


def leaves(dir_entry, name_id=None):
    for item in dir_entry.directory.entries:
        if item.struct.DataIsDirectory:
            # First directory level under the type IS the resource id.
            yield from leaves(item, item.id if name_id is None else name_id)
        else:
            leaf = item.data if hasattr(item, "data") else item
            s = leaf.struct
            yield name_id, pe.get_data(s.OffsetToData, s.Size)


def resources_of(type_id):
    out = []
    for entry in getattr(pe, "DIRECTORY_ENTRY_RESOURCE", None).entries:
        if entry.id != type_id:
            continue
        out.extend(leaves(entry))
    return out


groups = resources_of(RT_GROUP_ICON)
print(f"RT_GROUP_ICON resources: {len(groups)}")
assert groups, "no icon group in executable"

# Parse GRPICONDIR: reserved(2), type(2), count(2), then 14-byte entries.
gid, gdata = groups[0]
count = struct.unpack("<H", gdata[4:6])[0]
print(f"icon entries: {count}")
images = resources_of(RT_ICON)
by_id = dict(images)

ico = open(ico_path, "rb").read()
icount = struct.unpack("<H", ico[4:6])[0]
print(f"committed ico entries: {icount}")
assert count == icount, "exe icon group differs from committed ico"

ico_blobs = []
j = 6
for _ in range(icount):
    # ICONDIRENTRY (file): B,B,B,B,H,H,I(dwBytes),I(dwOffset) = 16 bytes.
    _w, _h, _cc, _res, _pl, _bi, size, data_off = struct.unpack("<BBBBHHII", ico[j:j + 16])
    ico_blobs.append(ico[data_off:data_off + size])
    j += 16

# Compare each embedded image against the committed ICO image blobs.
off = 6
matched = 0
for _ in range(count):
    w, h, cc, res, planes, bits, size, entry_id = struct.unpack("<BBBBHHIH", gdata[off:off + 14])
    exe_img = by_id[entry_id]
    assert any(exe_img == blob for blob in ico_blobs), f"embedded {w}x{h} image differs from committed ico"
    matched += 1
    off += 14
print(f"matched images: {matched}/{count}")
print("ICON EMBEDDING VERIFIED")
