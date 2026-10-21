; Front-to-back visibility for one eight-row band. NEXT/END describe holes
; keyed by their first x; 160 terminates each list. The retired command array
; supplies both 1280-byte arrays. RR_SCRATCH+12..19 hold the eight list heads
; until packing. I/O remains hidden; the IRQ never touches this workspace.
;
; A noncommitting raster prepass unions TC coverage into COVER's low nibble.
; Reverse painter order then claims each visible interval, including clear
; colour ownership. Exterior terrain cannot claim TC-covered pixels. Final
; nonbackground intervals reach cell_rows once in ordinary mode; COVER is
; already final. Resident helpers never call an overlay.
.zeropage __rc2
.zeropage __rc10
.zeropage __rc11
.ifndef SPAN_NEXT
.set SPAN_NEXT, $d000
.endif
.ifndef SPAN_END
.set SPAN_END, $d500
.endif
.if (<SPAN_NEXT) != (<SPAN_END)
.error "front span row pointer stepping requires matching low addresses"
.endif
.ifndef RR_SCRATCH
.set RR_SCRATCH, $77a0
.endif
.ifndef COVER_CLEAN
.set COVER_CLEAN, $7750
.endif
.set RR_PLAYER_SEEN, RR_SCRATCH+11
.set FRONT_HEADS, RR_SCRATCH+12
.set SF_NEXT, $e1
.set SF_END, $e3
.set SF_CUR, $e5
.set SF_PREV, $e6
.set SF_STOP, $e7
.set SF_LINK, $e8
.set SF_FIRST, $e9
.set SF_LAST, $ea
.set SF_SCAN, $eb
.set SF_LIMIT, $ec
.set SF_SAVE_FIRST, $ed
.set SF_SAVE_END, $ee
.set SF_MODE, $ef
.set CS_ROW, $f0
.set CS_FIRST, $f1
.set CS_END, $f2
.set CS_COLOR, $f3
.set CS_HUD, $f4
.set CS_MODE, $f5
.set SF_BITS, $f6
.set SF_BOUND, $f7
.set SF_MASK, $f8
.set SF_RIGHT, $f9
.set CS_KEY, $fe
.set CS_CLEAR, $ff

.section .text.span_front_row_pointers,"ax",@progbits
.globl span_front_row_pointers
.type span_front_row_pointers,@function
span_front_row_pointers:
    ldx CS_ROW
    lda .Lfront_next_lo,x
    sta SF_NEXT
    lda .Lfront_next_hi,x
    sta SF_NEXT+1
    lda .Lfront_end_lo,x
    sta SF_END
    lda .Lfront_end_hi,x
    sta SF_END+1
    rts
.Lfront_next_lo:
    .byte <(SPAN_NEXT+0), <(SPAN_NEXT+160), <(SPAN_NEXT+320), <(SPAN_NEXT+480)
    .byte <(SPAN_NEXT+640), <(SPAN_NEXT+800), <(SPAN_NEXT+960), <(SPAN_NEXT+1120)
.Lfront_next_hi:
    .byte >(SPAN_NEXT+0), >(SPAN_NEXT+160), >(SPAN_NEXT+320), >(SPAN_NEXT+480)
    .byte >(SPAN_NEXT+640), >(SPAN_NEXT+800), >(SPAN_NEXT+960), >(SPAN_NEXT+1120)
.Lfront_end_lo:
    .byte <(SPAN_END+0), <(SPAN_END+160), <(SPAN_END+320), <(SPAN_END+480)
    .byte <(SPAN_END+640), <(SPAN_END+800), <(SPAN_END+960), <(SPAN_END+1120)
.Lfront_end_hi:
    .byte >(SPAN_END+0), >(SPAN_END+160), >(SPAN_END+320), >(SPAN_END+480)
    .byte >(SPAN_END+640), >(SPAN_END+800), >(SPAN_END+960), >(SPAN_END+1120)
.size span_front_row_pointers, .-span_front_row_pointers

; CS span, COVER row rc10/11 already selected. Requires row <8 and nonempty
; clipped bounds 0<=first<end<=160. Run only in fresh cell state, before any
; scene writes/spills: this is a noncommitting union of all TC geometry.
; Preserves the high HUD nibble, all CS inputs, imaginary registers and walker
; state. Dirties COVER_CLEAN so the next cell clear resets the seeded union.
.section .text.span_front_cover_prepared,"ax",@progbits
.globl span_front_cover_prepared
.type span_front_cover_prepared,@function
span_front_cover_prepared:
    lda #1
    sta RR_PLAYER_SEEN
    lda #0
    sta COVER_CLEAN
    lda CS_END
    sec
    sbc #1
    tax
    lsr
    lsr
    sta SF_BOUND
    txa
    and #3
    tax
    lda .Lfront_right,x
    sta SF_RIGHT
    lda CS_FIRST
    and #3
    tax
    lda .Lfront_left,x
    sta SF_MASK
    lda CS_FIRST
    lsr
    lsr
    tay
    cpy SF_BOUND
    beq .Lfront_cover_last
    lda (__rc10),y
    ora SF_MASK
    sta (__rc10),y
    iny
.Lfront_cover_middle:
    cpy SF_BOUND
    beq .Lfront_cover_right
    lda (__rc10),y
    ora #15
    sta (__rc10),y
    iny
    bne .Lfront_cover_middle
.Lfront_cover_right:
    lda #15
    sta SF_MASK
.Lfront_cover_last:
    lda SF_MASK
    and SF_RIGHT
    ora (__rc10),y
    sta (__rc10),y
    rts
.Lfront_left:
    .byte 15, 7, 3, 1
.Lfront_right:
    .byte 8, 12, 14, 15
.size span_front_cover_prepared, .-span_front_cover_prepared

; Return the next accepted [SF_FIRST,SF_LAST) with carry set, removing it
; from the holes. SF_SCAN advances, SF_LIMIT and SF_MODE stay fixed. Carry
; clear means no accepted pixels remain. Uses only E1..EF and cell temporaries
; F6..F9; the overlay caller may freely call cell_rows between iterations.
.section .text.span_front_next,"ax",@progbits
.globl span_front_next
.type span_front_next,@function
span_front_next:
    lda SF_SCAN
    cmp SF_LIMIT
    bcc .Lfront_search
    clc
    rts
.Lfront_search:
    lda #255
    sta SF_PREV
    ldx CS_ROW
    ldy FRONT_HEADS,x
.Lfront_hole:
    cpy SF_LIMIT
    bcc .Lfront_intersect
    clc
    rts
.Lfront_intersect:
    sty SF_CUR
    lda (SF_END),y
    sta SF_STOP
    cmp SF_SCAN
    bcc .Lfront_skip
    beq .Lfront_skip
    cmp SF_LIMIT
    bcc .Lfront_limit
    lda SF_LIMIT
.Lfront_limit:
    sta SF_LAST
    tya
    cmp SF_SCAN
    bcs .Lfront_start
    lda SF_SCAN
.Lfront_start:
    sta SF_FIRST
    lda SF_MODE
    cmp #2
    bne .Lfront_claim
    jsr .Lfront_filter
    bcs .Lfront_claim
    ldy SF_CUR
.Lfront_skip:
    sty SF_PREV
    lda (SF_NEXT),y
    tay
    jmp .Lfront_hole
.Lfront_claim:
    lda SF_LAST
    sta SF_SCAN
    ldy SF_CUR
    lda (SF_NEXT),y
    sta SF_LINK
    lda SF_FIRST
    cmp SF_CUR
    bne .Lfront_keep_prefix
    lda SF_LAST
    cmp SF_STOP
    beq .Lfront_remove
    ; Cut a prefix: move the surviving hole's key to the accepted end.
    tay
    lda SF_STOP
    sta (SF_END),y
    lda SF_LINK
    sta (SF_NEXT),y
    tya
    bne .Lfront_relink
.Lfront_remove:
    lda SF_LINK
.Lfront_relink:
    ldy SF_PREV
    cpy #255
    bne .Lfront_link_prev
    ldx CS_ROW
    sta FRONT_HEADS,x
    sec
    rts
.Lfront_link_prev:
    sta (SF_NEXT),y
    sec
    rts
.Lfront_keep_prefix:
    sta (SF_END),y
    lda SF_LAST
    cmp SF_STOP
    beq .Lfront_claim_done
    ; Split around the accepted middle; the old node keeps the prefix.
    sta (SF_NEXT),y
    tay
    lda SF_STOP
    sta (SF_END),y
    lda SF_LINK
    sta (SF_NEXT),y
.Lfront_claim_done:
    sec
    rts

; Trim an exterior intersection to its first unprotected run. Empty full
; cells skip four pixels at once; partial masks use the four small bit tests.
.Lfront_filter:
    lda SF_LAST
    sta SF_BOUND
.Lfront_find_clear:
    lda SF_FIRST
    lsr
    lsr
    tay
    lda (__rc10),y
    and #15
    beq .Lfront_clear_found
    cmp #15
    beq .Lfront_skip_cell
    sta SF_BITS
    lda SF_FIRST
    and #3
    tax
    lda SF_BITS
    and .Lfront_bit,x
    beq .Lfront_clear_found
    lda SF_FIRST
    clc
    adc #1
    bne .Lfront_next_first
.Lfront_skip_cell:
    lda SF_FIRST
    and #252
    clc
    adc #4
.Lfront_next_first:
    sta SF_FIRST
    cmp SF_BOUND
    bcc .Lfront_find_clear
    clc
    rts
.Lfront_clear_found:
    lda SF_FIRST
    sta SF_LAST
.Lfront_extend:
    lda SF_LAST
    cmp SF_BOUND
    bcs .Lfront_filter_done
    lsr
    lsr
    tay
    lda (__rc10),y
    and #15
    beq .Lfront_clear_cell
    sta SF_BITS
    lda SF_LAST
    and #3
    tax
    lda SF_BITS
    and .Lfront_bit,x
    bne .Lfront_filter_done
    inc SF_LAST
    bne .Lfront_extend
.Lfront_clear_cell:
    lda SF_LAST
    and #252
    clc
    adc #4
    cmp SF_BOUND
    bcc .Lfront_extend_store
    lda SF_BOUND
.Lfront_extend_store:
    sta SF_LAST
    bne .Lfront_extend
.Lfront_filter_done:
    sec
    rts
.Lfront_bit:
    .byte 8, 4, 2, 1
.size span_front_next, .-span_front_next

.section .phase.pixels.front_spans,"ax",@progbits
.globl fast_span_clear
.type fast_span_clear,@function
fast_span_clear:
    lda #0
    sta CS_ROW
    sta RR_PLAYER_SEEN
.Lfront_clear_row:
    jsr span_front_row_pointers
    ldy #0
    lda #160
    sta (SF_NEXT),y
    sta (SF_END),y
    lda #0
    ldx CS_ROW
    sta FRONT_HEADS,x
    inc CS_ROW
    lda CS_ROW
    cmp #8
    bne .Lfront_clear_row
    rts
.size fast_span_clear, .-fast_span_clear

; Direct test/standalone entries prepare pointers; raster uses prepared
; entries and steps them with its rows. All preserve rc2..7, rc12..31 and
; D0..E0. Public entries select rc8..11; prepared entries preserve rc8..11 and
; E1..E4. A/X/Y, E5..EF and cell temporaries F6..FD are volatile.
;
; Both entries require CS_ROW<8, 0<=CS_FIRST<CS_END<=160. Cover has the fresh
; state requirements above. Compose requires the complete TC union first,
; initialized hole lists, CS_COLOR<16, CS_KEY=CS_COLOR XOR CS_CLEAR, CS_HUD=0,
; and CS_MODE 0 (plain), 1 (TC), or 2 (exterior). It claims clear-colour spans
; too, without emitting them. It preserves CS_ROW/FIRST/END/COLOR/HUD/MODE and
; KEY/CLEAR; ownership and final cells are the only persistent changes.
.globl span_front_cover
.type span_front_cover,@function
span_front_cover:
    jsr cell_row_pointers
    jmp span_front_cover_prepared
.size span_front_cover, .-span_front_cover
.globl span_front_compose
.type span_front_compose,@function
span_front_compose:
    jsr cell_row_pointers
    jsr span_front_row_pointers
.size span_front_compose, .-span_front_compose
.globl span_front_compose_prepared
.type span_front_compose_prepared,@function
span_front_compose_prepared:
    lda CS_FIRST
    sta SF_SAVE_FIRST
    sta SF_SCAN
    lda CS_END
    sta SF_SAVE_END
    sta SF_LIMIT
    lda CS_MODE
    sta SF_MODE
.Lfront_emit:
    jsr span_front_next
    bcc .Lfront_restore
    lda CS_KEY
    beq .Lfront_emit
    lda SF_FIRST
    sta CS_FIRST
    lda SF_LAST
    sta CS_END
    lda #0
    sta CS_MODE
    jsr cell_rows
    jmp .Lfront_emit
.Lfront_restore:
    lda SF_SAVE_FIRST
    sta CS_FIRST
    lda SF_SAVE_END
    sta CS_END
    lda SF_MODE
    sta CS_MODE
    rts
.size span_front_compose_prepared, .-span_front_compose_prepared

; C ABI row A, x X, colour rc2. Ambient dust claims holes after geometry.
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
    eor CS_CLEAR
    sta CS_KEY
    lda #0
    sta CS_HUD
    sta CS_MODE
    jmp span_front_compose
.size fast_span_plot, .-fast_span_plot
