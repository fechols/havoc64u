; Timed kernels with fixed nominal 6502 cycle counts. All are C ABI functions
; without arguments or results; they clobber only A, X and Y.

.text

; Interrupts stay off for the whole probe; the KERNAL is never re-entered.
.globl probe_disable_interrupts
probe_disable_interrupts:
    sei
    rts

; 256 x (ldx + 256 x (dex, bne)) nested loops: 329,215 nominal cycles for
; the loops themselves, 329,229 including ldy, jsr and rts. No data memory
; traffic, so it measures pure CPU speed.
.globl probe_cpu_loop
probe_cpu_loop:
    ldy #0
.Lcpu_outer:
    ldx #0
.Lcpu_inner:
    dex
    bne .Lcpu_inner
    dey
    bne .Lcpu_outer
    rts

; 1024 color-RAM writes: each sta $d800,x is an I/O-bus access.
.globl probe_write_color_ram
probe_write_color_ram:
    lda #1
    ldy #4
.Lcolor_page:
    ldx #0
.Lcolor_write:
    sta $d800,x
    inx
    bne .Lcolor_write
    dey
    bne .Lcolor_page
    rts

; 1024 writes to the border color register.
.globl probe_write_border
probe_write_border:
    lda #0
    ldy #4
.Lborder_page:
    ldx #0
.Lborder_write:
    sta $d020
    inx
    bne .Lborder_write
    dey
    bne .Lborder_page
    rts

; 1024 reads of the raster register, as frame publication polls it.
.globl probe_read_raster
probe_read_raster:
    ldy #4
.Lraster_page:
    ldx #0
.Lraster_read:
    lda $d012
    inx
    bne .Lraster_read
    dey
    bne .Lraster_page
    rts

; 1024 writes to ordinary RAM with the same addressing mode as color RAM.
.globl probe_write_ram
probe_write_ram:
    lda #1
    ldy #4
.Lram_page:
    ldx #0
.Lram_write:
    sta $6000,x
    inx
    bne .Lram_write
    dey
    bne .Lram_page
    rts

; Copy 8 KiB from $A000 to the page given in A (high byte), 14 nominal
; cycles per byte. The code runs from RAM, so the operands are patched.
.globl probe_copy_8k
probe_copy_8k:
    sta .Lcopy_store+2
    lda #$a0
    sta .Lcopy_load+2
    ldy #32
.Lcopy_page:
    ldx #0
.Lcopy_load:
    lda $a000,x
.Lcopy_store:
    sta $6000,x
    inx
    bne .Lcopy_load
    inc .Lcopy_load+2
    inc .Lcopy_store+2
    dey
    bne .Lcopy_page
    rts
