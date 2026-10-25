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
    ; Show the PRG-owned image before the CRT, REU checks, or Rust startup.
    ; Its initializer and source retire when the first overlay is loaded.
    jsr __havoc_show_splash

; Unlike the default C64 image, our .data VMA is below the PRG load address.
.section .init.251,"ax",@progbits
    jsr __havoc_copy_data

; Reassert the runtime mapping immediately before the call to main.
.section .init.999,"ax",@progbits
    lda #$35
    sta $01

; Both copy helpers retire with the splash before any overlay is loaded.
.section .splash_init,"ax",@progbits
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
; frame (the startup data copy runs before BSS and the stacks are ready).
.section .splash_init,"ax",@progbits
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

; This code shares the PRG's otherwise empty overlay window with the splash.
; Only the hardware stack and imaginary registers exist at this early point.
.section .splash_init,"ax",@progbits
.globl __havoc_show_splash
.type __havoc_show_splash,@function
__havoc_show_splash:
    ; Use the configured C64U speed for the initial copies. Main still checks
    ; that the turbo register is available and supplies the normal diagnostic.
    lda #$8f
    sta $d031
    lda #$2b
    sta $d011
    lda #$18
    sta $d016
    lda #$08
    sta $d018
    lda #0
    sta $d015
    sta $d020
    lda __splash_start+10000
    sta $d021
    lda $dd02
    ora #3
    sta $dd02
    lda $dd00
    and #$fc
    ora #1
    sta $dd00

    lda #<__splash_start
    sta __rc2
    lda #>__splash_start
    sta __rc3
    lda #<$a000
    sta __rc4
    lda #>$a000
    sta __rc5
    lda #<8000
    sta __rc6
    lda #>8000
    sta __rc7
    jsr __havoc_copy_bytes

    lda #<(__splash_start+8000)
    sta __rc2
    lda #>(__splash_start+8000)
    sta __rc3
    lda #<$8000
    sta __rc4
    lda #>$8000
    sta __rc5
    lda #<1000
    sta __rc6
    lda #>1000
    sta __rc7
    jsr __havoc_copy_bytes

    lda #<(__splash_start+9000)
    sta __rc2
    lda #>(__splash_start+9000)
    sta __rc3
    lda #<$d800
    sta __rc4
    lda #>$d800
    sta __rc5
    lda #<1000
    sta __rc6
    lda #>1000
    sta __rc7
    jsr __havoc_copy_bytes
    lda #$3b
    sta $d011
.globl __havoc_splash_ready
__havoc_splash_ready:
    rts
.size __havoc_show_splash, .-__havoc_show_splash

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
    ; Classify the scheduled raster event before touching compiler scratch.
    ; $D012 reads the current beam, not the programmed comparison, so retain
    ; the schedule explicitly. Never read/ack CIA ICR on the middle fast path:
    ; simultaneous audio stays pending and enters the ordinary IRQ after RTI.
    lda #0
    sta IRQ_FRAME_EVENT
    lda $d019
    and #1
    beq .Lirq_full
    sta $d019
    lda SPRITE_PHASE
    bne .Lirq_frame
    lda #1
    sta SPRITE_PHASE
    lda #251
    sta $d012
    jsr reticle_arm
    jmp .Lirq_return
.Lirq_frame:
    lda #0
    sta SPRITE_PHASE
    lda #92
    sta $d012
    inc IRQ_FRAME_EVENT
.Lirq_full:
    ldx #0
.Lirq_save:
    lda __rc0,x
    pha
    inx
    cpx #32
    bne .Lirq_save
    ; Interrupt Rust code runs on its own software stack. No function
    ; reachable from irq_body owns a compiler static frame (the effects
    ; engine is assembly), so the foreground's static locals need no backup;
    ; tools/check-irq-frames.py rejects a build that reintroduces one.
    lda #<__irq_stack
    sta __rc0
    lda #>__irq_stack
    sta __rc1
    lda IRQ_FRAME_EVENT
    jsr irq_body
    ldx #31
.Lirq_restore:
    pla
    sta __rc0,x
    dex
    bpl .Lirq_restore
.Lirq_return:
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

; C64GS second fire uses SID POTX, selected through CIA 1 PA6/PA7.
; Timer B runs at Phi2 even with the CPU accelerated. A keyboard scan changes
; the paddle selection, so keep the previous button state until 1024 hardware
; cycles have elapsed. Neither the IRQ nor the foreground waits for conversion.
.section .bss.controls,"aw",@nobits
.globl controls_pot_since
controls_pot_since:
    .space 2
.globl controls_pot_held
controls_pot_held:
    .space 1
controls_tmp:
    .space 1

; Called only with IRQs masked. PA0..PA5 remain inputs; PA7 selects port 2.
; Set the data latch high before changing direction to avoid driving any
; joystick line low, then timestamp the final paddle selection.
.section .text.restore_controls,"ax",@progbits
.globl restore_controls
restore_controls:
    lda #$ff
    sta $dc00
    lda #$c0
    sta $dc02
    lda #$80
    sta $dc00
.Lcontrols_restore_timer:
    lda $dc07
    ldx $dc06
    cmp $dc07
    bne .Lcontrols_restore_timer
    stx controls_pot_since
    sta controls_pot_since+1
    rts

; Return the six logical held bits in A. The low five are the ordinary
; active-low joystick inputs; bit 5 is POTX second fire OR Left Shift.
; Assembly keeps this IRQ path free of compiler static frames.
.section .text.read_controls,"ax",@progbits
.globl read_controls
read_controls:
    lda $dc00
    eor #$ff
    and #$1f
    pha
.Lcontrols_read_timer:
    lda $dc07
    tay
    ldx $dc06
    cmp $dc07
    bne .Lcontrols_read_timer
    stx controls_tmp
    lda controls_pot_since
    sec
    sbc controls_tmp
    sty controls_tmp
    lda controls_pot_since+1
    sbc controls_tmp
    cmp #4
    bcc .Lcontrols_shift
    lda $d419
    cmp #$80
    lda #0
    bcs .Lcontrols_store_pot
    lda #$20
.Lcontrols_store_pot:
    sta controls_pot_held
.Lcontrols_shift:
    lda #$ff
    sta $dc00
    sta $dc02
    lda #$fd
    sta $dc00
    lda $dc01
    asl
    lda #0
    bcs .Lcontrols_merge
    lda #$20
.Lcontrols_merge:
    ; Read the raw Shift column: idle qualification would reject a genuine
    ; Shift while joystick DOWN grounds its row. None of the other game keys
    ; share column 7. Unrelated multi-key combinations can still matrix-ghost.
    ora controls_pot_held
    sta controls_tmp
    pla
    ora controls_tmp
    pha
    jsr restore_controls
    pla
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
