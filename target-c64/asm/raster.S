; Persistent Q16 two-edge row walker. C ABI: job pointer in rc2/3.
; Job: head pointer, tail pointer, pixels pointer (unused), u8 band_y, u8 paint.
; Head16: i32 long_x,long_step,short_x,short_step.
; Tail12: i32 lower_x,lower_step; u8 top,middle,bottom,color_flags.
; flags bit4: long is left. Caller owns sequential bands and ordered masks.
; Native positions carry +32767 (fast_prepare_triangle adds it once), so a
; row's column is the high word of the position: no rounding add per row.
; The head is copied into zero page ($D0-$DF), stepped there and written back
; on exit (except from the triangle's last band, which no later band reads):
; the long step never changes and the short step only once the lower
; edge replaced it. The rows of a band split into the segment before the
; middle row and the segment from it, so no row tests for the middle; a
; painted row clamps both columns from the high words and hands the span to
; the cell writer, or a feature-gated span backend. The relevant cell/hole/run
; pointers step with painted rows; hidden rows only advance persistent edges.
; fast_raster_band drives the core for a whole band from its ordered mask.
; Front-only paint=2 is an internal noncommitting TC-union walk. Direct
; fast_raster_rows callers using 2 must supply TC geometry and fresh cells;
; paint=1 claims visible spans after the complete union has been seeded.
; fast_raster_band paint=1 performs both passes itself; paint=0 advances the
; persistent edges exactly once without coverage or scene writes.
; rc2..19, $D0-$EF and $77A0-$77B9 are volatile; IRQ never accesses them.
; $77AB is reset once per band, then retains whether a terrain-clear span was painted.
; Tail bit5 marks terrain-clear geometry; bit6 marks exterior terrain. Pixel bit6
; retains protected coverage through later ordinary foreground writes.
.zeropage __rc2
.zeropage __rc3
.zeropage __rc4
.zeropage __rc5
.zeropage __rc6
.zeropage __rc7
.zeropage __rc8
.zeropage __rc9
.zeropage __rc10
.zeropage __rc11
; Row-walker scratch (the mos-sim harness relocates the base).
.ifndef RR_SCRATCH
.set RR_SCRATCH, $77a0
.endif
.set RR_BAND, RR_SCRATCH+0
.set RR_PAINT, RR_SCRATCH+1
.set RR_ROW, RR_SCRATCH+2
.set RR_END, RR_SCRATCH+3
.set RR_MIDDLE, RR_SCRATCH+5
.set RR_FLAGS, RR_SCRATCH+6
.set RR_PLAYER_SEEN, RR_SCRATCH+11
.set RR_LOWERED, RR_SCRATCH+20
.set RR_INDEX, RR_SCRATCH+21
.set RR_MASK_BYTE, RR_SCRATCH+22
.set RR_MASK_BITS, RR_SCRATCH+23
; Persistent raster heads: sixteen bytes per sorted position, after the band
; masks (the mos-sim harness relocates the base).
.ifndef RASTER_HEADS
.set RASTER_HEADS, $ef20
.endif
.ifndef RASTER_TAILS
.set RASTER_TAILS, $e000
.endif
; The walker's zero page (PIXELS only; the compiler's zero-page region $22-$EF
; is empty, which check-layout.py asserts): the head's four Q16 values in head
; order, then the rows left in the current segment.
.set RW_LONG, $d0
.set RW_LONGSTEP, $d4
.set RW_SHORT, $d8
.set RW_SHORTSTEP, $dc
.set RW_ROWS, $e0
; Inputs of the cell writer (cells.S, zero page): band row, span bounds,
; colour and its key (colour XOR the band's clear colour), HUD flag and paint
; mode; the span section hands every painted span to cell_rows after
; cell_row_pointers selected the row.
.set CS_ROW, $f0
.set CS_FIRST, $f1
.set CS_END, $f2
.set CS_COLOR, $f3
.set CS_HUD, $f4
.set CS_MODE, $f5
.set CS_KEY, $fe
.set CS_CLEAR, $ff
.ifndef SPAN_PREPARED_POINTERS
.set SPAN_PREPARED_POINTERS, 1
.endif
.ifdef SPAN_BUFFER
.if SPAN_PREPARED_POINTERS
.set RASTER_SPAN_POINTERS, 1
.endif
.endif
.ifdef SPAN_FRONT
.set RASTER_SPAN_POINTERS, 1
.set RASTER_CELL_POINTERS, 1
.endif
.ifndef SPAN_BUFFER
.set RASTER_CELL_POINTERS, 1
.endif

.section .phase.pixels.raster,"ax",@progbits
.globl fast_raster_rows
fast_raster_rows:
    ldy #0
    lda (__rc2),y
    sta __rc4
    iny
    lda (__rc2),y
    sta __rc5
    iny
    lda (__rc2),y
    sta __rc6
    iny
    lda (__rc2),y
    sta __rc7
    ldy #6
    lda (__rc2),y
    sta RR_BAND
    iny
    lda (__rc2),y
    sta RR_PAINT
    ; Core: head rc4/5, tail rc6/7, RR_BAND and RR_PAINT.
.Lrr_core:
    lda RR_BAND
    sta RR_ROW
    clc
    adc #8
    sta RR_END
    ldy #8
    lda (__rc6),y
    cmp RR_ROW
    bcc .Lrr_top
    sta RR_ROW
.Lrr_top:
    iny
    lda (__rc6),y
    sta RR_MIDDLE
    iny
    lda (__rc6),y
    cmp RR_END
    bcs .Lrr_bottom
    sta RR_END
.Lrr_bottom:
    lda RR_ROW
    cmp RR_END
    bcc .Lrr_nonempty
    rts
.Lrr_nonempty:
    iny
    lda (__rc6),y
    sta RR_FLAGS
    and #15
    sta CS_COLOR
    eor CS_CLEAR
    sta CS_KEY
    ; The cell writer's inputs that hold for the whole triangle band: colour
    ; (and its key: colour XOR the clear colour), no HUD ink, and the paint
    ; mode (terrain-clear geometry paints and covers; exterior terrain skips
    ; covered pixels once a clear span was painted in this band — which only
    ; a clear triangle can change, so it is fixed here).
    lda #0
    sta CS_HUD
    sta CS_MODE
    lda RR_FLAGS
    and #$20
    beq .Lrr_mode_ordinary
    lda #1
    sta CS_MODE
    bne .Lrr_mode_set
.Lrr_mode_ordinary:
    lda RR_PLAYER_SEEN
    beq .Lrr_mode_set
    lda RR_FLAGS
    and #$40
    beq .Lrr_mode_set
    lda #2
    sta CS_MODE
.Lrr_mode_set:
    ; The cell writer's row and its CODES/COVER pointers (cells.S tables).
    sec
    lda RR_ROW
    sbc RR_BAND
    sta CS_ROW
.ifdef RASTER_SPAN_POINTERS
    lda RR_PAINT
    beq .Lrr_pointers_ready
.ifdef SPAN_FRONT
    jsr cell_row_pointers
    jsr span_front_row_pointers
.else
.if SPAN_PREPARED_POINTERS
    jsr span_row_pointers
.endif
.endif
.Lrr_pointers_ready:
.else
.ifndef SPAN_BUFFER
    jsr cell_row_pointers
.endif
.endif
    ; Bring the head into zero page.
    ldy #0
    lda (__rc4),y
    sta RW_LONG
    iny
    lda (__rc4),y
    sta RW_LONG+1
    iny
    lda (__rc4),y
    sta RW_LONG+2
    iny
    lda (__rc4),y
    sta RW_LONG+3
    iny
    lda (__rc4),y
    sta RW_LONGSTEP
    iny
    lda (__rc4),y
    sta RW_LONGSTEP+1
    iny
    lda (__rc4),y
    sta RW_LONGSTEP+2
    iny
    lda (__rc4),y
    sta RW_LONGSTEP+3
    iny
    lda (__rc4),y
    sta RW_SHORT
    iny
    lda (__rc4),y
    sta RW_SHORT+1
    iny
    lda (__rc4),y
    sta RW_SHORT+2
    iny
    lda (__rc4),y
    sta RW_SHORT+3
    iny
    lda (__rc4),y
    sta RW_SHORTSTEP
    iny
    lda (__rc4),y
    sta RW_SHORTSTEP+1
    iny
    lda (__rc4),y
    sta RW_SHORTSTEP+2
    iny
    lda (__rc4),y
    sta RW_SHORTSTEP+3
    lda #0
    sta RR_LOWERED
    ; Segments: the rows before the middle row, then the rows from it. The
    ; lower edge replaces the short edge exactly when the middle row lies in
    ; this band's rows (a middle row above them was replaced in an earlier band).
    lda RR_MIDDLE
    cmp RR_ROW
    bcc .Lrr_single
    cmp RR_END
    bcs .Lrr_single
    sec
    sbc RR_ROW
    beq .Lrr_lower
    sta RW_ROWS
    jsr .Lrr_segment
.Lrr_lower:
    ldy #0
    lda (__rc6),y
    sta RW_SHORT
    iny
    lda (__rc6),y
    sta RW_SHORT+1
    iny
    lda (__rc6),y
    sta RW_SHORT+2
    iny
    lda (__rc6),y
    sta RW_SHORT+3
    iny
    lda (__rc6),y
    sta RW_SHORTSTEP
    iny
    lda (__rc6),y
    sta RW_SHORTSTEP+1
    iny
    lda (__rc6),y
    sta RW_SHORTSTEP+2
    iny
    lda (__rc6),y
    sta RW_SHORTSTEP+3
    sty RR_LOWERED
    sec
    lda RR_END
    sbc RR_MIDDLE
    sta RW_ROWS
    jsr .Lrr_segment
    jmp .Lrr_exit
.Lrr_single:
    sec
    lda RR_END
    sbc RR_ROW
    sta RW_ROWS
    jsr .Lrr_segment
.Lrr_exit:
.ifdef SPAN_FRONT
    ; The TC union pass reads the persistent heads but never commits its
    ; temporary Q16 stepping or lower-edge transition.
    lda RR_PAINT
    cmp #2
    bne .Lrr_commit
    rts
.Lrr_commit:
.endif
    ; Persist the stepped positions unless this band held the triangle's
    ; last row (no later band reads the head); the long step is unchanged.
    ldy #10
    lda (__rc6),y
    cmp RR_END
    beq .Lrr_done
    ldy #0
    lda RW_LONG
    sta (__rc4),y
    iny
    lda RW_LONG+1
    sta (__rc4),y
    iny
    lda RW_LONG+2
    sta (__rc4),y
    iny
    lda RW_LONG+3
    sta (__rc4),y
    ldy #8
    lda RW_SHORT
    sta (__rc4),y
    iny
    lda RW_SHORT+1
    sta (__rc4),y
    iny
    lda RW_SHORT+2
    sta (__rc4),y
    iny
    lda RW_SHORT+3
    sta (__rc4),y
    lda RR_LOWERED
    beq .Lrr_done
    iny
    lda RW_SHORTSTEP
    sta (__rc4),y
    iny
    lda RW_SHORTSTEP+1
    sta (__rc4),y
    iny
    lda RW_SHORTSTEP+2
    sta (__rc4),y
    iny
    lda RW_SHORTSTEP+3
    sta (__rc4),y
.Lrr_done:
    rts

; Walk RW_ROWS (>= 1) rows: unpainted bands only advance the edges; painted
; bands take the loop whose left edge matches the orientation flag.
.Lrr_segment:
    lda RR_PAINT
    beq .Lrr_walk_only
    lda RR_FLAGS
    and #16
    beq .Lrr_short_left
    jmp .Lrr_rows_long_left
.Lrr_short_left:
    jmp .Lrr_rows_short_left
.Lrr_walk_only:
    clc
    lda RW_LONG
    adc RW_LONGSTEP
    sta RW_LONG
    lda RW_LONG+1
    adc RW_LONGSTEP+1
    sta RW_LONG+1
    lda RW_LONG+2
    adc RW_LONGSTEP+2
    sta RW_LONG+2
    lda RW_LONG+3
    adc RW_LONGSTEP+3
    sta RW_LONG+3
    clc
    lda RW_SHORT
    adc RW_SHORTSTEP
    sta RW_SHORT
    lda RW_SHORT+1
    adc RW_SHORTSTEP+1
    sta RW_SHORT+1
    lda RW_SHORT+2
    adc RW_SHORTSTEP+2
    sta RW_SHORT+2
    lda RW_SHORT+3
    adc RW_SHORTSTEP+3
    sta RW_SHORT+3
    dec RW_ROWS
    bne .Lrr_walk_only
    rts

; One painted row loop per orientation. A row clamps the left and right
; columns from the biased positions' high words (0..160), hands a nonempty
; span [first, end) to cell_rows, advances both edges and steps the cell
; writer's row (the COVER row only matters to the terrain modes).
.macro RASTER_ROWS name, left, lstep, right, rstep
.Lrr_rows_\name:
    lda \left+3
    bmi .Lrr_left_zero_\name
    bne .Lrr_skip_\name
    lda \left+2
    cmp #160
    bcs .Lrr_skip_\name
    bcc .Lrr_left_ok_\name
.Lrr_left_zero_\name:
    lda #0
.Lrr_left_ok_\name:
    sta CS_FIRST
    lda \right+3
    bmi .Lrr_skip_\name
    bne .Lrr_right_full_\name
    lda \right+2
    cmp #160
    bcc .Lrr_right_ok_\name
.Lrr_right_full_\name:
    lda #160
.Lrr_right_ok_\name:
    cmp CS_FIRST
    bcc .Lrr_skip_\name
    beq .Lrr_skip_\name
    sta CS_END
.ifdef SPAN_BUFFER
.ifdef SPAN_FRONT
    lda RR_PAINT
    cmp #2
    bne .Lrr_front_claim_\name
    jsr span_front_cover_prepared
    jmp .Lrr_skip_\name
.Lrr_front_claim_\name:
    jsr span_front_compose_prepared
.else
.if SPAN_PREPARED_POINTERS
    jsr span_compose_prepared
.else
    jsr span_compose
.endif
.endif
.else
    ; A painted, nonempty terrain-clear span marks the band (mode 1 only).
    lda CS_MODE
    lsr
    bcc .Lrr_cells_\name
    lda #1
    sta RR_PLAYER_SEEN
.Lrr_cells_\name:
    jsr cell_rows
.endif
.Lrr_skip_\name:
    clc
    lda \left
    adc \lstep
    sta \left
    lda \left+1
    adc \lstep+1
    sta \left+1
    lda \left+2
    adc \lstep+2
    sta \left+2
    lda \left+3
    adc \lstep+3
    sta \left+3
    clc
    lda \right
    adc \rstep
    sta \right
    lda \right+1
    adc \rstep+1
    sta \right+1
    lda \right+2
    adc \rstep+2
    sta \right+2
    lda \right+3
    adc \rstep+3
    sta \right+3
    ; next band row: the cell writer's CODES and COVER rows are 40 bytes on
    ; (stepped after the segment's last row too, harmlessly: every triangle
    ; band derives the pointers afresh, and the second segment continues)
    inc CS_ROW
.ifdef RASTER_SPAN_POINTERS
    ; Both arrays have the same low address and rows are 160 bytes apart.
    ; The pointers also advance after clipped-empty rows and between the two
    ; edge segments; hidden bands take the separate advance-only loop.
    lda $e1
    clc
    adc #160
    sta $e1
    sta $e3
    bcc .Lrr_span_row_\name
    inc $e2
    inc $e4
.Lrr_span_row_\name:
.endif
.ifdef RASTER_CELL_POINTERS
    lda __rc8
    clc
    adc #40
    sta __rc8
    bcc .Lrr_codes_row_\name
    inc __rc9
.Lrr_codes_row_\name:
.ifndef SPAN_FRONT
    lda CS_MODE
    beq .Lrr_count_\name
.endif
    lda __rc10
    clc
    adc #40
    sta __rc10
    bcc .Lrr_count_\name
    inc __rc11
.endif
.Lrr_count_\name:
    dec RW_ROWS
    beq .Lrr_rows_done_\name
    jmp .Lrr_rows_\name
.Lrr_rows_done_\name:
    rts
.endm
RASTER_ROWS long_left, RW_LONG, RW_LONGSTEP, RW_SHORT, RW_SHORTSTEP
RASTER_ROWS short_left, RW_SHORT, RW_SHORTSTEP, RW_LONG, RW_LONGSTEP

; void fast_raster_band(const uint8_t *mask, uint8_t *pixels, uint8_t band_y,
;                       uint8_t paint)
; C ABI: mask rc2/3, pixels rc4/5 (unused: the cell writer owns the fixed
; pixel band), band_y A, paint X. Walks every triangle whose bit is set in
; the band's ordered 32-byte mask, in index order, with its persistent head
; at RASTER_HEADS + 16 i and prepared tail at RASTER_TAILS + 12 i. The front
; backend first walks only TC geometry without committing heads, then walks
; all geometry in reverse mask order; hidden bands skip the coverage prepass.
; Replaces the per-triangle Job marshalling of the Rust band loop.
.globl fast_raster_band
fast_raster_band:
    sta RR_BAND
    stx RR_PAINT
.ifdef SPAN_FRONT
    cpx #0
    bne .Lrb_prepass
    jmp .Lrb_reverse_start
.Lrb_prepass:
    lda #2
    sta RR_PAINT
.endif
    lda #0
    sta RR_INDEX
    sta RR_MASK_BYTE
.Lrb_byte:
    ldy RR_MASK_BYTE
    lda (__rc2),y
    bne .Lrb_have_bits
    ; Eight unused indices.
    clc
    lda RR_INDEX
    adc #8
    sta RR_INDEX
    jmp .Lrb_next_byte
.Lrb_have_bits:
    sta RR_MASK_BITS
.Lrb_bit:
    lsr RR_MASK_BITS
    bcs .Lrb_triangle
    lda RR_MASK_BITS
    beq .Lrb_rest_clear
    ; Bits remain, so this bit was below seven and the index stays in the byte.
    inc RR_INDEX
    bne .Lrb_bit
.Lrb_rest_clear:
    ; No further bits in this byte: continue at the next byte's first index.
    lda RR_INDEX
    ora #7
    clc
    adc #1
    sta RR_INDEX
    jmp .Lrb_next_byte
.Lrb_triangle:
    ; head = RASTER_HEADS + 16 i (the heads follow the masks at $EF20)
    lda RR_INDEX
    lsr
    lsr
    lsr
    lsr
    sta __rc5
    lda RR_INDEX
    and #15
    asl
    asl
    asl
    asl
    clc
    adc #<RASTER_HEADS
    sta __rc4
    lda __rc5
    adc #>RASTER_HEADS
    sta __rc5
    ; tail = $E000 + 12 i = $E000 + 8 i + 4 i
    lda RR_INDEX
    lsr
    lsr
    lsr
    lsr
    lsr
    sta __rc7
    lda RR_INDEX
    asl
    asl
    asl
    sta __rc6
    lda RR_INDEX
    lsr
    lsr
    lsr
    lsr
    lsr
    lsr
    clc
    adc __rc7
    sta __rc7
    lda RR_INDEX
    asl
    asl
    clc
    adc __rc6
    sta __rc6
    lda __rc7
    adc #>RASTER_TAILS
    sta __rc7
.if <RASTER_TAILS
    clc
    lda __rc6
    adc #<RASTER_TAILS
    sta __rc6
    bcc .Lrb_tail_ready
    inc __rc7
.Lrb_tail_ready:
.endif
.ifdef SPAN_FRONT
    lda RR_PAINT
    cmp #2
    bne .Lrb_run_core
    ldy #11
    lda (__rc6),y
    and #$20
    beq .Lrb_after_core
.Lrb_run_core:
.endif
    jsr .Lrr_core
.Lrb_after_core:
.ifdef SPAN_FRONT
    lda RR_PAINT
    cmp #2
    beq .Lrb_forward_continue
    jmp .Lrb_reverse_continue
.Lrb_forward_continue:
.endif
    lda RR_MASK_BITS
    bne .Lrb_more_bits
    jmp .Lrb_rest_clear
.Lrb_more_bits:
    inc RR_INDEX
    jmp .Lrb_bit
.Lrb_next_byte:
    inc RR_MASK_BYTE
    lda RR_MASK_BYTE
    cmp #32
    beq .Lrb_done
    jmp .Lrb_byte
.Lrb_done:
.ifdef SPAN_FRONT
    lda #1
    sta RR_PAINT
.Lrb_reverse_start:
    lda #255
    sta RR_INDEX
    lda #31
    sta RR_MASK_BYTE
.Lrb_reverse_byte:
    ldy RR_MASK_BYTE
    lda (__rc2),y
    bne .Lrb_reverse_have_bits
    sec
    lda RR_INDEX
    sbc #8
    sta RR_INDEX
    jmp .Lrb_reverse_next_byte
.Lrb_reverse_have_bits:
    sta RR_MASK_BITS
.Lrb_reverse_bit:
    asl RR_MASK_BITS
    bcc .Lrb_reverse_continue
    jmp .Lrb_triangle
.Lrb_reverse_continue:
    lda RR_MASK_BITS
    beq .Lrb_reverse_rest_clear
    dec RR_INDEX
    jmp .Lrb_reverse_bit
.Lrb_reverse_rest_clear:
    lda RR_INDEX
    and #248
    sec
    sbc #1
    sta RR_INDEX
.Lrb_reverse_next_byte:
    dec RR_MASK_BYTE
    bmi .Lrb_reverse_done
    jmp .Lrb_reverse_byte
.Lrb_reverse_done:
.endif
    rts
