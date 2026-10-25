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
    names={0x14:'beql',0x15:'bnel',0x0c:'andi',0x29:'sh',0x28:'sb',0x24:'lbu',0x20:'lb',0x27:'lwu',0x06:'blez',0x07:'bgtz',0x09:'addiu',0x0d:'ori',0x0f:'lui',0x23:'lw',0x2b:'sw',0x3f:'sd',0x37:'ld',0x21:'lh',0x25:'lhu',0x19:'daddiu',0x1e:'lq',0x1f:'sq',0x0a:'slti',0x04:'beq',0x05:'bne',0x31:'lwc1',0x39:'swc1'}
    if op==3: return f'jal {((w&0x3ffffff)<<2):#x}'
    if op==2: return f'j {((w&0x3ffffff)<<2):#x}'
    if op==1:
        k={0:'bltz',1:'bgez',2:'bltzl',3:'bgezl'}.get(rt,f'regimm{rt}')
        return f'{k} {R[rs]},{a+4+simm*4:#x}'
    if op==0:
        f=w&63
        if f==0x25 or f==0x2d: return f'or/move {R[rd]},{R[rs]},{R[rt]}'
        if f==8: return f'jr {R[rs]}'
        sp={0x24:'and',0x21:'addu',0x2d:'daddu',0x23:'subu',0x2a:'slt',0x2b:'sltu',0x26:'xor',0x27:'nor',0x0a:'movz',0x0b:'movn'}
        if f in sp: return f'{sp[f]} {R[rd]},{R[rs]},{R[rt]}'
        if f in (0,2,3,0x38,0x3a,0x3b,0x3c,0x3e,0x3f):
            sh={0:'sll',2:'srl',3:'sra',0x38:'dsll',0x3a:'dsrl',0x3b:'dsra',0x3c:'dsll32',0x3e:'dsrl32',0x3f:'dsra32'}[f]
            return 'nop' if w==0 else f'{sh} {R[rd]},{R[rt]},{(w>>6)&31}'
        return f'special f={f:#x} {R[rd]},{R[rs]},{R[rt]}'
    if op in names:
        n=names[op]
        if n in('lui',): return f'lui {R[rt]},{imm:#x}'
        if n in('addiu','daddiu','slti'): return f'{n} {R[rt]},{R[rs]},{simm}'
        if n in('ori','andi'): return f'{n} {R[rt]},{R[rs]},{imm:#x}'
        if n in('blez','bgtz'): return f'{n} {R[rs]},{a+4+simm*4:#x}'
        if n in('beq','bne','beql','bnel'): return f'{n} {R[rs]},{R[rt]},{a+4+simm*4:#x}'
        return f'{n} {R[rt]},{simm}({R[rs]})'
    return f'op{op:#x} {w:#010x}'
lo,hi=int(sys.argv[2],16),int(sys.argv[3],16)
for a in range(lo,hi,4): print(f'{a:#x}  {dis(a)}')
