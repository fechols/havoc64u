; Original H.A.V.O.C. hardware glue, using the LLVM-MOS C calling convention.
; All interrupt-owned Rust code uses a separate software stack. -mlto-zp=0 is
; mandatory: only the fixed $02-$21 imaginary registers are saved below.

.zeropage __rc0
.zeropage __rc1
.zeropage __rc2
.zeropage __rc3
.zeropage __rc4
.zeropage __rc5
.zeropage __rc6
.zeropage __rc7

; The SDK runs this before stack setup (.100), BSS clearing (.200), and main.
.section .init.000,"ax",@progbits
    sei
    cld
    ldx #$ff
    txs
    lda #$7f
    sta $dc0d
    sta $dd0d
    lda $dc0d
    lda $dd0d
    lda #0
    sta $d01a
    lda #$0f
    sta $d019
    lda #<nmi_entry
    sta $fffa
    lda #>nmi_entry
    sta $fffb
    lda #<irq_entry
    sta $fffe
    lda #>irq_entry
    sta $ffff
    lda #$2f
    sta $00
    ; The linker discards the SDK's ROM CHROUT hook. BSS owns KERNAL vector
    ; RAM, so the complete remaining startup runs with both ROMs disabled.
    lda #$35
    sta $01

; Unlike the default C64 image, our .data VMA is below the PRG load address.
.section .init.251,"ax",@progbits
    jsr __havoc_copy_data

; Reassert the runtime mapping immediately before the call to main.
.section .init.999,"ax",@progbits
    lda #$35
    sta $01

.section .text.__havoc_copy_data,"ax",@progbits
.globl __havoc_copy_data
__havoc_copy_data:
    lda #<__data_load_start
    sta __rc2
    lda #>__data_load_start
    sta __rc3
    lda #<__data_start
    sta __rc4
    lda #>__data_start
    sta __rc5
    lda #<__data_size
    sta __rc6
    lda #>__data_size
    sta __rc7
    jmp __havoc_copy_bytes

; Copy rc6/7 bytes from rc2/3 to rc4/5 without touching any compiler static
; frame. The page loop keeps the interrupt scratch backup cheap at 64 MHz.
.section .text.__havoc_copy_bytes,"ax",@progbits
__havoc_copy_bytes:
    ldy #0
    ldx __rc7
    beq .Lcopy_data_tail
.Lcopy_data_page:
    lda (__rc2),y
    sta (__rc4),y
    iny
    bne .Lcopy_data_page
    inc __rc3
    inc __rc5
    dex
    bne .Lcopy_data_page
.Lcopy_data_tail:
    cpy __rc6
    beq .Lcopy_data_done
    lda (__rc2),y
    sta (__rc4),y
    iny
    jmp .Lcopy_data_tail
.Lcopy_data_done:
    rts

.section .text.irq_entry,"ax",@progbits
.globl irq_entry
irq_entry:
    pha
    txa
    pha
    tya
    pha
    lda $01
    pha
    lda #$35
    sta $01
    cld
    ldx #0
.Lirq_save:
    lda __rc0,x
    pha
    inx
    cpx #32
    bne .Lirq_save
    lda #<__irq_static_save_start
    sta __rc2
    lda #>__irq_static_save_start
    sta __rc3
    lda #<__irq_static_backup
    sta __rc4
    lda #>__irq_static_backup
    sta __rc5
    lda #<__irq_static_first_size
    sta __rc6
    lda #>__irq_static_first_size
    sta __rc7
    jsr __havoc_copy_bytes
    lda #<__irq_static_tail_start
    sta __rc2
    lda #>__irq_static_tail_start
    sta __rc3
    lda #<__irq_static_backup_tail
    sta __rc4
    lda #>__irq_static_backup_tail
    sta __rc5
    lda #<__irq_static_tail_size
    sta __rc6
    lda #>__irq_static_tail_size
    sta __rc7
    jsr __havoc_copy_bytes
    lda #<__irq_stack
    sta __rc0
    lda #>__irq_stack
    sta __rc1
    jsr irq_body
    lda #<__irq_static_backup
    sta __rc2
    lda #>__irq_static_backup
    sta __rc3
    lda #<__irq_static_save_start
    sta __rc4
    lda #>__irq_static_save_start
    sta __rc5
    lda #<__irq_static_first_size
    sta __rc6
    lda #>__irq_static_first_size
    sta __rc7
    jsr __havoc_copy_bytes
    lda #<__irq_static_backup_tail
    sta __rc2
    lda #>__irq_static_backup_tail
    sta __rc3
    lda #<__irq_static_tail_start
    sta __rc4
    lda #>__irq_static_tail_start
    sta __rc5
    lda #<__irq_static_tail_size
    sta __rc6
    lda #>__irq_static_tail_size
    sta __rc7
    jsr __havoc_copy_bytes
    ldx #31
.Lirq_restore:
    pla
    sta __rc0,x
    dex
    bpl .Lirq_restore
    pla
    sta $01
    pla
    tay
    pla
    tax
    pla
    rti

.section .text.nmi_entry,"ax",@progbits
.globl nmi_entry
nmi_entry:
    rti

.section .text.disable_irq,"ax",@progbits
.globl disable_irq
disable_irq:
    sei
    rts

.section .text.enable_irq,"ax",@progbits
.globl enable_irq
enable_irq:
    cli
    rts

; External-call barriers keep compiler-generated triangle accesses on the
; intended side of the processor-port change. The IRQ preserves this setting.
.section .text.io_off,"ax",@progbits
.globl io_off
io_off:
    lda #$34
    sta $01
    rts

.section .text.io_on,"ax",@progbits
.globl io_on
io_on:
    lda #$35
    sta $01
    rts

; Four interleaved 250-byte strips publish exactly 1000 color cells. The
; caller chooses a safe border interval and masks IRQs across publication.
.section .text.fast_copy_color,"ax",@progbits
.globl fast_copy_color
fast_copy_color:
    ldx #0
.Lcopy_color:
    lda $9100,x
    sta $d800,x
    lda $91fa,x
    sta $d8fa,x
    lda $92f4,x
    sta $d9f4,x
    lda $93ee,x
    sta $daee,x
    inx
    cpx #250
    bne .Lcopy_color
    rts

; void fast_fill_span(uint8_t *destination, uint8_t count, uint8_t color)
; Verified MOS C ABI: pointer __rc2/__rc3, count A, color X. Count 0 is empty;
; all other byte counts are supported, including a full 160-pixel scanline.
.section .text.fast_fill_span,"ax",@progbits
.globl fast_fill_span
fast_fill_span:
    tay
    beq .Lspan_done
    txa
.Lspan_fill:
    dey
    sta (__rc2),y
    cpy #0
    bne .Lspan_fill
.Lspan_done:
    rts
