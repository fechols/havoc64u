; Eight persistent VIC sprites, grouped into four two-image instruments.
; Publisher ABI: corner 0..3 in A, images at $8E00..$8E7F, colour at $8E80.
; I/O must be mapped. Staging survives the call; rc2..15 are caller scratch.
; Only bytes that differ from the visible images are written.
.zeropage __rc2
.zeropage __rc3
.zeropage __rc4
.zeropage __rc5
.zeropage __rc6
.zeropage __rc7
.zeropage __rc8
.zeropage __rc9
.zeropage __rc12
.zeropage __rc13

.section .phase.pixels.hud_publish,"ax",@progbits
.globl publish_hud_corner
.type publish_hud_corner,@function
publish_hud_corner:
    php
    cld
    sta __rc7
    asl
    sta __rc6
    lda __rc7
    lsr
    clc
    adc #$8c
    sta __rc5
    lda __rc7
    and #1
    lsr
    ror
    sta __rc4
    ; Top sprites begin at Y=62, bottom at Y=225. Guard every possible
    ; opposite fine-scroll offset (-4..+3), with at least six leading lines and
    ; trailing fetch margin. The measured copy bound is below two lines.
    lda __rc7
    cmp #2
    bcs .Lhud_bottom
    lda #52
    sta __rc8
    lda #92
    bne .Lhud_end
.Lhud_bottom:
    lda #212
    sta __rc8
    lda #254
.Lhud_end:
    sta __rc9
.Lhud_retry:
    cli
.globl hud_publish_wait
hud_publish_wait:
.Lhud_wait:
    jsr .Lhud_safe
    bcc .Lhud_wait
    sei
.globl hud_publish_guard
hud_publish_guard:
    jsr .Lhud_safe
    bcc .Lhud_retry
.globl hud_publish_commit
hud_publish_commit:
    ldy #0
.Lhud_copy:
    lda $8e00,y
    cmp (__rc4),y
    beq .Lhud_next
    sta (__rc4),y
    inc HUD_IMAGE_WRITES
    bne .Lhud_next
    inc HUD_IMAGE_WRITES+1
.Lhud_next:
    iny
    cpy #128
    bne .Lhud_copy
    ldx __rc6
    bne .Lhud_first_color
    ; Sprite zero belongs to the reticle during the middle of the frame.
    ; Never overwrite its live foreground color while publishing score pixels.
    lda $8e80
    sta HUD_SPRITE0_COLOR
    jmp .Lhud_second_color
.Lhud_first_color:
    lda $d027,x
    and #15
    cmp $8e80
    beq .Lhud_second_color
    lda $8e80
    sta $d027,x
.Lhud_second_color:
    lda $d028,x
    and #15
    cmp $8e80
    beq .Lhud_count
    lda $8e80
    sta $d028,x
.Lhud_count:
    inc HUD_COMMITS
    bne .Lhud_done
    inc HUD_COMMITS+1
.Lhud_done:
.globl hud_publish_done
hud_publish_done:
    plp
    rts

; High/low/high sampling rejects raster rollover. IRQs may run during the
; initial wait, but cannot invalidate the final sample and bounded copy.
.Lhud_safe:
    lda $d011
    and #$80
    sta __rc12
    lda $d012
    sta __rc13
    lda $d011
    and #$80
    cmp __rc12
    bne .Lhud_safe
    lda __rc12
    bne .Lhud_yes
    lda __rc13
    cmp __rc8
    bcc .Lhud_yes
    cmp __rc9
    bcs .Lhud_yes
    clc
    rts
.Lhud_yes:
    sec
    rts
.size publish_hud_corner, .-publish_hud_corner

; Cancel before painting a caption, including after this refresh's line-92
; arm. This is the only foreground sprite-register update: phase and enable
; are read with interrupts masked, so it cannot hide the top HUD occurrence.
.section .phase.pixels.reticle_hide,"ax",@progbits
.globl reticle_hide
.type reticle_hide,@function
reticle_hide:
    php
    sei
    lda #0
    sta RETICLE_PENDING_VISIBLE
    sta RETICLE_ACTIVE_VISIBLE
    lda SPRITE_PHASE
    beq .Lreticle_hide_done
    lda $d015
    and #$fe
    sta $d015
.Lreticle_hide_done:
.globl reticle_hide_done
reticle_hide_done:
    plp
    rts
.size reticle_hide, .-reticle_hide

; Resident: the line-251 IRQ supplies the bitmap's packed scroll sample.
; Normal bitmap scroll is (0,3). Subtract its displacement from each sprite's
; fixed base for equal, opposite cockpit motion without accumulated drift.
.section .text.hud_apply_scroll,"ax",@progbits
.globl hud_apply_scroll
.type hud_apply_scroll,@function
hud_apply_scroll:
    sta HUD_SCROLL
    pha
    and #7
    sta __rc2
    pla
    lsr
    lsr
    lsr
    sec
    sbc #3
    sta __rc3
    ldx #0
    ldy #0
.Lhud_position:
    lda .Lhud_base_x,x
    sec
    sbc __rc2
    sta $d000,y
    lda .Lhud_base_y,x
    sec
    sbc __rc3
    sta $d001,y
    inx
    iny
    iny
    cpx #8
    bne .Lhud_position
    ; Right corners are X=290/314, left are X=30/54. Subtracting X scroll 0..7
    ; cannot cross a 256-pixel boundary, so all nine-bit X values use $CC.
    lda #$cc
    sta $d010
    rts
.size hud_apply_scroll, .-hud_apply_scroll

; Shared resident data; leave scarce constant RAM for game/font records.
.section .text.hud_positions,"a",@progbits
.Lhud_base_x:
    .byte 30,54,34,58,30,54,34,58
.Lhud_base_y:
    .byte 62,62,62,62,225,225,225,225

; Called only by the line-92 fast IRQ, with A/X/Y and $01 already saved.
; No imaginary register or compiler static scratch is touched. The upper HUD
; has finished DMA by line 92 even at maximum shake. Arm only with at least
; eight lines left before the reticle's Y comparison; an unexpectedly late
; interrupt hides this occurrence instead of risking a partial/wrong image.
.section .text.reticle_arm,"ax",@progbits
.globl reticle_arm
.type reticle_arm,@function
reticle_arm:
    lda $d015
    and #$fe
    sta $d015
    lda RETICLE_ACTIVE_VISIBLE
    beq .Lreticle_arm_done
    lda HUD_SCROLL
    lsr
    lsr
    lsr
    sec
    sbc #3
    clc
    adc RETICLE_ACTIVE_Y
    sta $d001
    ; High/low/high sampling conservatively rejects ninth-bit rollover.
    lda $d011
    bmi .Lreticle_late
    ldy $d012
    lda $d011
    bmi .Lreticle_late
    cpy #92
    bcc .Lreticle_late
    tya
    clc
    adc #8
    bcs .Lreticle_late
    cmp $d001
    beq .Lreticle_arm_safe
    bcs .Lreticle_late
.Lreticle_arm_safe:
    lda HUD_SCROLL
    and #7
    clc
    adc RETICLE_ACTIVE_X
    sta $d000
    lda RETICLE_ACTIVE_X+1
    adc #0
    and #1
    tax
    lda $d010
    and #$fe
    cpx #0
    beq .Lreticle_high_x
    ora #1
.Lreticle_high_x:
    sta $d010
    lda #$ff ; linker asserts __reticle_sprite_start == $BFC0 in VIC bank 2
    sta $83f8
    lda $d01c
    and #$fe
    sta $d01c
    lda #1
    sta $d027
    lda $d015
    ora #1
    sta $d015
    bne .Lreticle_arm_done
.Lreticle_late:
    lda #0
    sta RETICLE_ACTIVE_VISIBLE
    inc RETICLE_MISSES
    bne .Lreticle_arm_done
    inc RETICLE_MISSES+1
.Lreticle_arm_done:
.globl reticle_arm_done
reticle_arm_done:
    rts
.size reticle_arm, .-reticle_arm

; The full line-251 IRQ has applied this frame's bitmap fine scroll and called
; hud_apply_scroll to restore all positions. Restore the borrowed image/mode
; and latch one completed render's aim. Foreground cannot interleave here.
.section .text.reticle_restore,"ax",@progbits
.globl reticle_restore
.type reticle_restore,@function
reticle_restore:
    lda $d015
    and #$fe
    sta $d015
    lda #$30
    sta $83f8
    lda $d01c
    ora #1
    sta $d01c
    lda HUD_SPRITE0_COLOR
    sta $d027
    lda RETICLE_PENDING_X
    sta RETICLE_ACTIVE_X
    lda RETICLE_PENDING_X+1
    sta RETICLE_ACTIVE_X+1
    lda RETICLE_PENDING_Y
    sta RETICLE_ACTIVE_Y
    lda RETICLE_PENDING_VISIBLE
    sta RETICLE_ACTIVE_VISIBLE
    lda HUD_ENABLE_PENDING
    sta HUD_ENABLE_ACTIVE
    sta $d015
.globl reticle_restore_done
reticle_restore_done:
    rts
.size reticle_restore, .-reticle_restore
