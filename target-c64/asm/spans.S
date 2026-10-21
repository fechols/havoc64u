; Deferred band compositor. A run starts at its own x coordinate; NEXT gives
; its exclusive end / successor, and INK keeps colour plus terrain coverage
; (bit 6). Every row starts at node zero and ends at sentinel 160. Only live
; nodes matter, so initialization writes one node per row, not 2560 bytes.
; The command array is retired after triangle preparation. I/O must be hidden
; throughout composition; the IRQ saves/restores its mapping and never uses
; this workspace or $E1-$E9. The persistent Q16 edge walker is unchanged.
;
; span_compose: CS_ROW/FIRST/END/COLOR/MODE describe a nonempty clipped span.
; Preserves all imaginary registers, the walker's $D0-$E0, all CS inputs and
; raster scratch. Clobbers A/X/Y and $E1-$E9. No calls into an overlay: this
; self-contained hot operation resides in ordinary resident code.
.zeropage __rc2
.ifndef SPAN_NEXT
.set SPAN_NEXT, $d000
.endif
.ifndef SPAN_INK
.set SPAN_INK, $d500
.endif
.set SP_NEXT, $e1
.set SP_INK, $e3
.set SP_PREV, $e5
.set SP_CUR, $e6
.set SP_STOP, $e7
.set SP_OLD, $e8
.set SP_NEW, $e9
.set CS_ROW, $f0
.set CS_FIRST, $f1
.set CS_END, $f2
.set CS_COLOR, $f3
.set CS_HUD, $f4
.set CS_MODE, $f5
.set CS_KEY, $fe
.set CS_CLEAR, $ff

.section .text.span_row_pointers,"ax",@progbits
.globl span_row_pointers
.type span_row_pointers,@function
span_row_pointers:
    ldx CS_ROW
    lda .Lspan_next_lo,x
    sta SP_NEXT
    lda .Lspan_next_hi,x
    sta SP_NEXT+1
    lda .Lspan_ink_lo,x
    sta SP_INK
    lda .Lspan_ink_hi,x
    sta SP_INK+1
    rts
.Lspan_next_lo:
    .byte <(SPAN_NEXT+0), <(SPAN_NEXT+160), <(SPAN_NEXT+320), <(SPAN_NEXT+480)
    .byte <(SPAN_NEXT+640), <(SPAN_NEXT+800), <(SPAN_NEXT+960), <(SPAN_NEXT+1120)
.Lspan_next_hi:
    .byte >(SPAN_NEXT+0), >(SPAN_NEXT+160), >(SPAN_NEXT+320), >(SPAN_NEXT+480)
    .byte >(SPAN_NEXT+640), >(SPAN_NEXT+800), >(SPAN_NEXT+960), >(SPAN_NEXT+1120)
.Lspan_ink_lo:
    .byte <(SPAN_INK+0), <(SPAN_INK+160), <(SPAN_INK+320), <(SPAN_INK+480)
    .byte <(SPAN_INK+640), <(SPAN_INK+800), <(SPAN_INK+960), <(SPAN_INK+1120)
.Lspan_ink_hi:
    .byte >(SPAN_INK+0), >(SPAN_INK+160), >(SPAN_INK+320), >(SPAN_INK+480)
    .byte >(SPAN_INK+640), >(SPAN_INK+800), >(SPAN_INK+960), >(SPAN_INK+1120)
.size span_row_pointers, .-span_row_pointers

.section .text.span_compose,"ax",@progbits
.globl span_compose
.type span_compose,@function
span_compose:
    jsr span_row_pointers
    lda #$ff
    sta SP_PREV
    ldy #0
.Lspan_find:
    lda CS_FIRST
    cmp (SP_NEXT),y
    bcc .Lspan_found
    sty SP_PREV
    lda (SP_NEXT),y
    tay
    jmp .Lspan_find
.Lspan_found:
    sty SP_CUR
    cpy CS_FIRST
    beq .Lspan_update
    ; Split the run containing first. Its old start remains the predecessor.
    lda (SP_NEXT),y
    sta SP_STOP
    lda (SP_INK),y
    sta SP_OLD
    lda CS_FIRST
    sta (SP_NEXT),y
    sty SP_PREV
    tay
    sty SP_CUR
    lda SP_STOP
    sta (SP_NEXT),y
    lda SP_OLD
    sta (SP_INK),y
.Lspan_update:
    ldy SP_CUR
    lda (SP_NEXT),y
    sta SP_STOP
    cmp CS_END
    bcc .Lspan_paint
    beq .Lspan_paint
    ; Preserve the untouched suffix when this is the final intersecting run.
    lda (SP_INK),y
    sta SP_OLD
    lda CS_END
    sta (SP_NEXT),y
    tay
    lda SP_STOP
    sta (SP_NEXT),y
    lda SP_OLD
    sta (SP_INK),y
    lda CS_END
    sta SP_STOP
    ldy SP_CUR
.Lspan_paint:
    lda (SP_INK),y
    ldx CS_MODE
    beq .Lspan_plain
    dex
    beq .Lspan_clear
    ; Exterior terrain cannot replace protected ink. Ordinary foreground can
    ; replace its colour, but preserves coverage for subsequent terrain.
    and #$40
    bne .Lspan_covered
    beq .Lspan_color
.Lspan_clear:
    lda #$40
    bne .Lspan_color
.Lspan_plain:
    and #$40
.Lspan_color:
    ora CS_COLOR
    sta (SP_INK),y
    jmp .Lspan_new
.Lspan_covered:
    lda (SP_INK),y
.Lspan_new:
    sta SP_NEW
    ldy SP_PREV
    cpy #$ff
    beq .Lspan_advance
    lda (SP_INK),y
    cmp SP_NEW
    bne .Lspan_advance
    ; Equal neighbours share a single node; dead nodes need no reclamation.
    lda SP_STOP
    sta (SP_NEXT),y
    sty SP_CUR
.Lspan_advance:
    lda SP_STOP
    cmp CS_END
    beq .Lspan_suffix
    ldx SP_CUR
    stx SP_PREV
    sta SP_CUR
    jmp .Lspan_update
.Lspan_suffix:
    cmp #160
    beq .Lspan_done
    tay
    lda (SP_INK),y
    cmp SP_NEW
    bne .Lspan_done
    lda (SP_NEXT),y
    ldy SP_CUR
    sta (SP_NEXT),y
.Lspan_done:
    rts
.size span_compose, .-span_compose

.section .phase.pixels.spans,"ax",@progbits
; void fast_span_clear(void): cell clear has already supplied CS_CLEAR.
.globl fast_span_clear
.type fast_span_clear,@function
fast_span_clear:
    lda #0
    sta CS_ROW
.Lspan_clear_row:
    jsr span_row_pointers
    ldy #0
    lda #160
    sta (SP_NEXT),y
    lda CS_CLEAR
    sta (SP_INK),y
    inc CS_ROW
    lda CS_ROW
    cmp #8
    bne .Lspan_clear_row
    rts
.size fast_span_clear, .-fast_span_clear

; void fast_span_plot(uint8_t row, uint8_t x, uint8_t color)
; C ABI: row A, x X, colour rc2. Ambient ink has no HUD priority.
.globl fast_span_plot
.type fast_span_plot,@function
fast_span_plot:
    sta CS_ROW
    stx CS_FIRST
    inx
    stx CS_END
    lda __rc2
    and #15
    sta CS_COLOR
    lda #0
    sta CS_MODE
    jmp span_compose
.size fast_span_plot, .-fast_span_plot

; void fast_span_flush(void): paint the final disjoint runs once, retaining
; their terrain protection in the cells. Only unprotected clear-colour runs
; already exist in the empty cells; protected background still needs a write.
.globl fast_span_flush
.type fast_span_flush,@function
fast_span_flush:
    lda #0
    sta CS_ROW
    sta CS_MODE
    sta CS_HUD
.Lspan_flush_row:
    jsr span_row_pointers
    jsr cell_row_pointers
    lda #0
    sta SP_CUR
.Lspan_flush_run:
    ldy SP_CUR
    sty CS_FIRST
    lda (SP_NEXT),y
    sta SP_STOP
    sta CS_END
    lda (SP_INK),y
    sta SP_OLD
    ldx #0
    and #$40
    beq .Lspan_flush_mode
    inx
.Lspan_flush_mode:
    stx CS_MODE
    lda SP_OLD
    and #15
    sta CS_COLOR
    eor CS_CLEAR
    sta CS_KEY
    lda SP_OLD
    cmp CS_CLEAR
    beq .Lspan_flush_next
    jsr cell_rows
.Lspan_flush_next:
    lda SP_STOP
    sta SP_CUR
    cmp #160
    bne .Lspan_flush_run
    inc CS_ROW
    lda CS_ROW
    cmp #8
    bne .Lspan_flush_row
    rts
.size fast_span_flush, .-fast_span_flush
