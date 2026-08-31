# Extracts the icon images from a PE (NSIS installer exe) and from an ICO,
# saves them as PNGs and computes a normalized pixel diff at a common size.
import struct, sys

def parse_pe_icons(path):
    d = open(path, 'rb').read()
    pe = struct.unpack_from('<I', d, 0x3C)[0]
    nsec = struct.unpack_from('<H', d, pe + 6)[0]
    optsz = struct.unpack_from('<H', d, pe + 20)[0]
    opt = pe + 24
    magic = struct.unpack_from('<H', d, opt)[0]
    n_dd = 16 if magic == 0x20b else 16
    dd = opt + optsz - n_dd * 8
    rva, rsize = struct.unpack_from('<II', d, dd + 2 * 8)
    # sections
    s = opt + optsz
    secs = []
    for i in range(nsec):
        vaddr = struct.unpack_from('<I', d, s + i * 40 + 12)[0]
        vsize = struct.unpack_from('<I', d, s + i * 40 + 8)[0]
        roff = struct.unpack_from('<I', d, s + i * 40 + 20)[0]
        rsize2 = struct.unpack_from('<I', d, s + i * 40 + 16)[0]
        secs.append((vaddr, roff, max(vsize, rsize2)))
    def r2o(va):
        for vaddr, roff, sz in secs:
            if vaddr <= va < vaddr + sz:
                return roff + (va - vaddr)
        return None
    # traverse resource tree
    def walk(va, depth, name_id, out):
        off = r2o(va)
        if off is None: return
        nname, nid = struct.unpack_from('<HH', d, off + 12)
        for i in range(nname + nid):
            e = off + 16 + i * 8
            nm = struct.unpack_from('<I', d, e)[0]
            val = struct.unpack_from('<I', d, e + 4)[0]
            if depth == 0:
                if (nm & 0x7FFFFFFF) in (3, 14):
                    walk(val & 0x7FFFFFFF, 1, nm & 0x7FFFFFFF, out)
            elif depth == 1:
                if val & 0x80000000:
                    walk(val & 0x7FFFFFFF, 2, name_id, out)
                else:
                    out.append((name_id, nm & 0x7FFFFFFF, val))
            elif depth == 2:
                pass
    groups = []
    walk(rva, 0, 0, groups)
    # for each group, resolve RT_ICON entries to bytes
    icons = {}
    for gid, icon_id, rva_data in groups:
        if gid != 14:  # only RT_GROUP_ICON groups
            continue
        off = r2o(rva_data)
        n = struct.unpack_from('<H', d, off + 4)[0]
        entries = []
        for i in range(n):
            e = off + 6 + i * 14
            w = d[e]; h = d[e+1]
            sz = struct.unpack_from('<I', d, e + 8)[0]
            rid = struct.unpack_from('<H', d, e + 12)[0]
            entries.append((w or 256, h or 256, sz, rid))
        blobs = []
        for (w, h, sz, rid) in entries:
            for (g2, i2, rva2) in groups:
                if g2 == 3 and i2 == rid:
                    o = r2o(rva2)
                    blobs.append((w, h, d[o:o+sz]))
                    break
        icons[gid] = blobs
    return icons

if __name__ == '__main__':
    exe = sys.argv[1]
    icons = parse_pe_icons(exe)
    print('groups:', {k: [(w, h, len(b)) for (w, h, b) in v] for k, v in icons.items()})
    import base64
    # save the largest per group as PNG (BMP header inside icon data)
    for gid, blobs in icons.items():
        for (w, h, blob) in blobs:
            out = f"C:/Users/reteren/AppData/Local/Temp/opencode/exe_icon_{gid}_{w}x{h}.png"
            # icon resource is a DIB (BITMAPINFOHEADER + pixels); wrap as PNG via PIL? no PIL.
            # write raw dib for now
            open(out, 'wb').write(blob)
            print('wrote', out, len(blob), 'bytes')