# Decompresses the NSIS solid LZMA block of a Tauri NSIS installer and searches
# the decompressed script data for the strings that must be present (the
# clipboard-history checkbox label, the registry paths, the uninstall prompt).
# Static only: nothing is executed.
import sys, struct, lzma

exe = sys.argv[1]
data = open(exe, 'rb').read()

# locate the NSIS data: the "NullsoftInst" signature marks the first block,
# the "NsisBlock" signature the solid compressed block.
nib = data.find(b'NullsoftInst')
print('NullsoftInst at offset', nib)
nsb = data.find(b'NsisBlock')
print('NsisBlock at offset', nsb)
if nsb < 0:
    sys.exit('no NsisBlock found')

# structure (NSIS exehead/fileform.h NsisBlock):
#   DWORD sf_head_flags; QWORD sf_size; DWORD sf_blocksize; DWORD sf_checksum;
#   DWORD sf_head_len; BYTE sf_head[sf_head_len]; then compressed payload.
o = nsb + len(b'NsisBlock')
flags = struct.unpack_from('<I', data, o)[0]; o += 4
size = struct.unpack_from('<Q', data, o)[0]; o += 8
blocksize = struct.unpack_from('<I', data, o)[0]; o += 4
checksum = struct.unpack_from('<I', data, o)[0]; o += 4
head_len = struct.unpack_from('<I', data, o)[0]; o += 4
head = data[o:o + head_len]; o += head_len
print(f'flags=0x{flags:x} size={size} blocksize={blocksize} checksum=0x{checksum:x} head_len={head_len} head={head.hex()}')

# LZMA1 properties: 1 byte lc/lp/pb + 4-byte dictionary size (LE)
props = head
lc_lp_pb = props[0]
lc = lc_lp_pb % 9
lp = (lc_lp_pb // 9) % 5
pb = lc_lp_pb // 45
dict_size = struct.unpack('<I', props[1:5])[0]
print(f'lzma lc={lc} lp={lp} pb={pb} dict={dict_size}')

filters = [{"id": lzma.FILTER_LZMA1, "lc": lc, "lp": lp, "pb": pb, "dict_size": dict_size}]
payload = data[o:]
dec = lzma.LZMADecompressor(format=lzma.FORMAT_RAW, filters=filters)
out = dec.decompress(payload)
print('decompressed bytes:', len(out))

targets = [
    'Disable Windows clipboard history (Win+V) for the current user',
    'Software\\Microsoft\\Clipboard',
    'EnableClipboardHistory',
    'ClipboardHistoryDisabledByRebuffer',
    'ClipboardHistoryWasEnabled',
    'Software\\reteren\\Rebuffer',
    'Rebuffer disabled Windows clipboard history (Win+V) during installation. Re-enable it now?',
    'rebuffer_restore_clipboard_history',
    'RebufferWelcomeShow',
    'rebuffer.exe',
    'System.dll',
    'modern-wizard.bmp',
]
for t in targets:
    a = t.encode('utf-8')
    w = t.encode('utf-16-le')
    print(f"{'FOUND ' if a in out else '----  '}ascii : {t}")
    print(f"{'FOUND ' if w in out else '----  '}utf16 : {t}")