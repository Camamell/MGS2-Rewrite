"""Quick MIPS (EE) disassembly of a stage's resident image, with no Ghidra:
the common integer ops only (enough for argument set-up and stack stores).
Usage: python3 -I re/tools/mips_res.py <resident.bin> <start> <end> (hex)
The resident loads at 0x1f1480 (re/notes/stage-dat.md)."""
import struct,sys
d=open(sys.argv[1],'rb').read()
B=0x1f1480
R=['zero','at','v0','v1','a0','a1','a2','a3','t0','t1','t2','t3','t4','t5','t6','t7','s0','s1','s2','s3','s4','s5','s6','s7','t8','t9','k0','k1','gp','sp','fp','ra']
def dis(a):
    w=struct.unpack_from('<I',d,a-B)[0]
    op=w>>26; rs=(w>>21)&31; rt=(w>>16)&31; rd=(w>>11)&31; imm=w&0xffff; simm=imm-0x10000 if imm&0x8000 else imm
    names={0x09:'addiu',0x0d:'ori',0x0f:'lui',0x23:'lw',0x2b:'sw',0x3f:'sd',0x37:'ld',0x21:'lh',0x25:'lhu',0x19:'daddiu',0x1e:'lq',0x1f:'sq',0x0a:'slti',0x04:'beq',0x05:'bne',0x31:'lwc1',0x39:'swc1'}
    if op==3: return f'jal {((w&0x3ffffff)<<2):#x}'
    if op==2: return f'j {((w&0x3ffffff)<<2):#x}'
    if op==0:
        f=w&63
        if f==0x25 or f==0x2d: return f'or/move {R[rd]},{R[rs]},{R[rt]}'
        if f==8: return f'jr {R[rs]}'
        return f'special f={f:#x} {R[rd]},{R[rs]},{R[rt]}'
    if op in names:
        n=names[op]
        if n in('lui',): return f'lui {R[rt]},{imm:#x}'
        if n in('addiu','daddiu','slti','ori'): return f'{n} {R[rt]},{R[rs]},{simm}'
        if n in('beq','bne'): return f'{n} {R[rs]},{R[rt]},{a+4+simm*4:#x}'
        return f'{n} {R[rt]},{simm}({R[rs]})'
    return f'op{op:#x} {w:#010x}'
lo,hi=int(sys.argv[2],16),int(sys.argv[3],16)
for a in range(lo,hi,4): print(f'{a:#x}  {dis(a)}')
