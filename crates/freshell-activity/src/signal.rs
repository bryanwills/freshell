//! Port of `shared/turn-complete-signal.ts` (frozen parity reference).
//!
//! The turn-complete signal is a bare BEL (`\x07`) the provider CLIs emit at a
//! positive turn end (claude via the Stop-hook bell `--settings` payload,
//! codex via `tui.notification_method=bel` + `tui.notifications=
//! ['agent-turn-complete']` — both already installed by
//! `freshell-platform::cli_launch`). BELs inside OSC/DCS escape sequences are
//! terminators/payload, not signals; a stray bell sandwiched between visible
//! output (a sub-tool ringing) is not a tracker-eligible signal either.

pub const TURN_COMPLETE_SIGNAL: char = '\u{07}';
const ESC: char = '\u{1b}';
const C1_ST: char = '\u{9c}';
const C1_CSI: char = '\u{9b}';
const C1_DCS: char = '\u{90}';
const C1_OSC: char = '\u{9d}';

/// `TurnCompleteSignalParserState` — carried across output chunks so an escape
/// sequence split over two PTY reads is still recognized.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ParserState {
    pub in_osc: bool,
    pub in_csi: bool,
    pub in_dcs: bool,
    pub pending_esc: bool,
}

impl ParserState {
    pub fn new() -> Self {
        Self::default()
    }
}

/// `isSubmitInput` (`shared/turn-complete-signal.ts:125-127`): the input is
/// ONLY a run of CR/LF bytes — an Enter keypress, possibly repeated.
pub fn is_submit_input(data: &str) -> bool {
    !data.is_empty() && data.chars().all(|c| c == '\r' || c == '\n')
}

/// JS `/[\u0000-\u001f\u007f-\u009f]/` — C0 + DEL + C1 controls.
fn is_control(ch: char) -> bool {
    matches!(ch, '\u{0000}'..='\u{001f}' | '\u{007f}'..='\u{009f}')
}

/// `isIgnorableLeadingTurnCompleteChar`: whitespace or control chars (other
/// than the BEL itself) never count as "visible output" around a signal.
/// JS `/\s/` additionally matches U+FEFF (ZWNBSP), which Rust's
/// `char::is_whitespace` does not — matched explicitly.
fn is_ignorable_leading_char(ch: char) -> bool {
    ch != TURN_COMPLETE_SIGNAL && (ch.is_whitespace() || ch == '\u{feff}' || is_control(ch))
}

/// `countTrackerTurnCompleteSignals`: counts only BELs that are
/// "tracker-eligible" — leading (no visible output before it in the chunk) or
/// with no visible output after it. Reads (copies) the parser state without
/// mutating it, exactly like the reference.
pub fn count_tracker_turn_complete_signals(data: &str, state: &ParserState) -> usize {
    let mut in_osc = state.in_osc;
    let mut pending_esc = state.pending_esc;
    let mut in_csi = state.in_csi;
    let mut in_dcs = state.in_dcs;
    let mut saw_visible_output = false;

    struct Candidate {
        leading_eligible: bool,
        has_visible_after: bool,
    }
    let mut candidates: Vec<Candidate> = Vec::new();

    for ch in data.chars() {
        if pending_esc {
            if in_osc && ch == '\\' {
                in_osc = false;
            } else if in_dcs && ch == '\\' {
                in_dcs = false;
            } else if !in_osc && !in_dcs && ch == ']' {
                in_osc = true;
            } else if !in_osc && !in_dcs && ch == '[' {
                in_csi = true;
            } else if !in_osc && !in_dcs && ch == 'P' {
                in_dcs = true;
            }
            pending_esc = false;
            continue;
        }

        if ch == ESC {
            pending_esc = true;
            continue;
        }

        if in_osc {
            if ch == TURN_COMPLETE_SIGNAL || ch == C1_ST {
                in_osc = false;
            }
            continue;
        }

        if in_dcs {
            if ch == C1_ST {
                in_dcs = false;
            }
            continue;
        }

        if in_csi {
            if ('@'..='~').contains(&ch) {
                in_csi = false;
            }
            continue;
        }

        if ch == C1_CSI {
            in_csi = true;
            continue;
        }
        if ch == C1_DCS {
            in_dcs = true;
            continue;
        }
        if ch == C1_OSC {
            in_osc = true;
            continue;
        }
        if ch == TURN_COMPLETE_SIGNAL {
            candidates.push(Candidate {
                leading_eligible: !saw_visible_output,
                has_visible_after: false,
            });
            continue;
        }
        if is_ignorable_leading_char(ch) {
            continue;
        }
        // Visible output: marks every prior candidate as "has visible after".
        saw_visible_output = true;
        for candidate in &mut candidates {
            candidate.has_visible_after = true;
        }
    }

    candidates
        .iter()
        .filter(|c| c.leading_eligible || !c.has_visible_after)
        .count()
}

/// Does this terminal mode carry the turn-complete BEL contract?
fn supports_turn_signal(mode: &str) -> bool {
    mode == "claude" || mode == "codex"
}

/// `extractTurnCompleteSignals`: strips bare turn-complete BELs from the
/// output (returning the cleaned text) and counts them, updating `state`
/// across chunks. For non-signal modes the data passes through unchanged
/// (with the reference's pending-ESC reset quirk preserved).
pub fn extract_turn_complete_signals(
    data: &str,
    mode: &str,
    state: &mut ParserState,
) -> (String, usize) {
    if !supports_turn_signal(mode) {
        if state.pending_esc {
            state.pending_esc = false;
            state.in_osc = false;
            state.in_csi = false;
            state.in_dcs = false;
            return (format!("{ESC}{data}"), 0);
        }
        return (data.to_string(), 0);
    }

    let mut in_osc = state.in_osc;
    let mut in_csi = state.in_csi;
    let mut in_dcs = state.in_dcs;
    let mut pending_esc = state.pending_esc;
    let mut cleaned = String::with_capacity(data.len());
    let mut count = 0usize;

    for ch in data.chars() {
        if pending_esc {
            if in_osc && ch == '\\' {
                cleaned.push(ESC);
                cleaned.push('\\');
                in_osc = false;
            } else if in_dcs && ch == '\\' {
                cleaned.push(ESC);
                cleaned.push('\\');
                in_dcs = false;
            } else if !in_osc && !in_dcs && ch == ']' {
                cleaned.push(ESC);
                cleaned.push(']');
                in_osc = true;
            } else if !in_osc && !in_dcs && ch == '[' {
                cleaned.push(ESC);
                cleaned.push('[');
                in_csi = true;
            } else if !in_osc && !in_dcs && ch == 'P' {
                cleaned.push(ESC);
                cleaned.push('P');
                in_dcs = true;
            } else {
                cleaned.push(ESC);
                cleaned.push(ch);
            }
            pending_esc = false;
            continue;
        }

        if ch == ESC {
            pending_esc = true;
            continue;
        }

        if ch == C1_CSI {
            cleaned.push(ch);
            in_csi = true;
            continue;
        }
        if ch == C1_DCS {
            cleaned.push(ch);
            in_dcs = true;
            continue;
        }
        if ch == C1_OSC {
            cleaned.push(ch);
            in_osc = true;
            continue;
        }

        if in_csi {
            cleaned.push(ch);
            if ('@'..='~').contains(&ch) {
                in_csi = false;
            }
            continue;
        }

        if ch == TURN_COMPLETE_SIGNAL {
            if in_osc {
                cleaned.push(ch);
                in_osc = false;
            } else if in_dcs {
                cleaned.push(ch);
            } else {
                count += 1;
            }
            continue;
        }

        if ch == C1_ST {
            if in_osc {
                cleaned.push(ch);
                in_osc = false;
            } else if in_dcs {
                cleaned.push(ch);
                in_dcs = false;
            } else {
                cleaned.push(ch);
            }
            continue;
        }

        if in_dcs {
            cleaned.push(ch);
            continue;
        }

        cleaned.push(ch);
    }

    state.in_osc = in_osc;
    state.in_csi = in_csi;
    state.in_dcs = in_dcs;
    state.pending_esc = pending_esc;
    (cleaned, count)
}

/// #612: deterministic quit-intent detection on freshell's own PTY input
/// stream (tapped at freshell-terminal registry.rs:1338). Rules (exact,
/// stated): Ctrl+C/Ctrl+D outside paste framing are immediate quit
/// intents; a submitted line equal to "/quit" or "/exit" (after
/// trimming) is a quit intent — including a PASTED one evaluated by a
/// later real Enter (both agent TUIs enable DECSET 2004, so xterm.js
/// frames every paste as ESC[200~…ESC[201~; the framing is unwrapped
/// here and never poisons); well-formed DECRQM reports
/// (ESC [ ? digits ; digit $ y — freshell's own client injects them as
/// synthetic input replies) are consumed without poisoning; any OTHER
/// escape sequence or control byte poisons the line buffer until the
/// next newline (TUI-menu quits are NOT detectable here — that residual
/// stays agent-evidence-dependent, idle.rs entry 11).
const QUIT_LINE_CAP: usize = 32;

#[derive(Debug, Default)]
pub struct QuitIntentState {
    line: String,
    unmatchable: bool,
    /// Inside xterm.js bracketed-paste framing (ESC[200~ … ESC[201~).
    in_paste: bool,
    /// Partial escape run carried across chunk boundaries while we
    /// decide whether it is a paste marker or a DECRQM report.
    pending_esc: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputClass {
    /// Ctrl+C / Ctrl+D (outside paste framing), or a submitted line
    /// equal to /quit or /exit — including a pasted one evaluated by a
    /// later real Enter.
    QuitIntent,
    /// A submitted line that is NOT a quit command.
    NonQuitSubmit,
    /// Anything else (typing, escape sequences, paste content, partial
    /// chunks).
    Other,
}

/// What a partially-accumulated escape run currently is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EscMatch {
    /// A proper prefix of a recognized sequence — keep accumulating
    /// (this is what tolerates markers split across input chunks:
    /// paste atomicity is observed, not contractual).
    Prefix,
    PasteBegin,
    PasteEnd,
    /// A complete DECRQM report — consume WITHOUT poisoning.
    Decrqm,
    /// Cannot become any recognized sequence: unrecognized escape —
    /// the poison rule applies.
    Fail,
}

/// Exact-grammar matcher for the three escape sequences the classifier
/// understands: ESC[200~, ESC[201~, and ESC [ ? <digits> ; <digit> $ y.
/// Deterministic — an exact grammar, not a general escape parser.
fn match_esc(seq: &str) -> EscMatch {
    const BEGIN: &str = "\u{1b}[200~";
    const END: &str = "\u{1b}[201~";
    if seq == BEGIN {
        return EscMatch::PasteBegin;
    }
    if seq == END {
        return EscMatch::PasteEnd;
    }
    let paste_prefix = BEGIN.starts_with(seq) || END.starts_with(seq);
    // DECRQM state machine; st == 8 means a complete report.
    let mut st = 0u8;
    let mut decrqm = true;
    for c in seq.chars() {
        st = match (st, c) {
            (0, '\u{1b}') => 1,
            (1, '[') => 2,
            (2, '?') => 3,
            (3, d) if d.is_ascii_digit() => 4,
            (4, d) if d.is_ascii_digit() => 4,
            (4, ';') => 5,
            (5, d) if d.is_ascii_digit() => 6,
            (6, '$') => 7,
            (7, 'y') => 8,
            _ => {
                decrqm = false;
                break;
            }
        };
    }
    if decrqm && st == 8 {
        return EscMatch::Decrqm;
    }
    if paste_prefix || (decrqm && st < 8) {
        return EscMatch::Prefix;
    }
    EscMatch::Fail
}

pub fn classify_input(state: &mut QuitIntentState, data: &str) -> InputClass {
    let mut class = InputClass::Other;
    for c in data.chars() {
        // --- escape accumulator (paste markers + DECRQM), runs first ---
        if !state.pending_esc.is_empty() {
            state.pending_esc.push(c);
            match match_esc(&state.pending_esc) {
                EscMatch::Prefix => continue,
                EscMatch::PasteBegin => {
                    state.pending_esc.clear();
                    state.in_paste = true; // framing itself never poisons
                    continue;
                }
                EscMatch::PasteEnd => {
                    state.pending_esc.clear();
                    state.in_paste = false; // normal classification resumes
                    continue;
                }
                EscMatch::Decrqm => {
                    // Synthetic client reply (request-mode-bypass.ts:256)
                    // — consumed, no poison.
                    state.pending_esc.clear();
                    continue;
                }
                EscMatch::Fail => {
                    // Unrecognized escape: the poison rule — then this
                    // final char is REPROCESSED below (a trailing \r
                    // must still evaluate; a fresh ESC restarts the
                    // accumulator).
                    state.pending_esc.clear();
                    state.line.clear();
                    state.unmatchable = true;
                }
            }
        }
        if c == '\u{1b}' {
            state.pending_esc.push(c);
            continue;
        }
        if state.in_paste {
            // Inside bracketed-paste framing: EVERYTHING is literal
            // pasted data.
            match c {
                '\r' | '\n' => {
                    // A literal pasted newline: a multi-line blob is not
                    // a submit — drop the finished line; the LAST pasted
                    // line remains evaluable by a later real Enter.
                    state.line.clear();
                    state.unmatchable = false;
                }
                c if c >= ' ' && c != '\u{7f}' => {
                    if state.line.len() >= QUIT_LINE_CAP {
                        state.unmatchable = true;
                    } else if !state.unmatchable {
                        state.line.push(c);
                    }
                }
                _ => {
                    // Pasted control bytes — including 0x03/0x04 — are
                    // DATA, not quit gestures; they also make the pasted
                    // line meaningless.
                    state.line.clear();
                    state.unmatchable = true;
                }
            }
            continue;
        }
        match c {
            '\u{3}' | '\u{4}' => {
                state.line.clear();
                state.unmatchable = false;
                class = InputClass::QuitIntent;
            }
            '\r' | '\n' => {
                let line = state.line.trim();
                let is_quit = !state.unmatchable && (line == "/quit" || line == "/exit");
                // A quit anywhere in the chunk wins over a later submit.
                if is_quit {
                    class = InputClass::QuitIntent;
                } else if class != InputClass::QuitIntent && (!line.is_empty() || state.unmatchable)
                {
                    // A poisoned line still SUBMITTED something — the
                    // user is driving the pane, so this counts as a
                    // NonQuitSubmit (marker clears). A bare Enter on a
                    // clean empty buffer stays Other.
                    class = InputClass::NonQuitSubmit;
                }
                state.line.clear();
                state.unmatchable = false;
            }
            '\u{7f}' | '\u{8}' => {
                state.line.pop();
            }
            c if c >= ' ' => {
                if state.line.len() >= QUIT_LINE_CAP {
                    state.unmatchable = true;
                } else if !state.unmatchable {
                    state.line.push(c);
                }
            }
            _ => {
                // Other control bytes: the buffer no longer represents
                // the visible line.
                state.line.clear();
                state.unmatchable = true;
            }
        }
    }
    class
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_submit_input_matches_the_reference_regex() {
        assert!(is_submit_input("\r"));
        assert!(is_submit_input("\n"));
        assert!(is_submit_input("\r\n"));
        assert!(is_submit_input("\r\r\n\n"));
        assert!(!is_submit_input(""));
        assert!(!is_submit_input("a\r"));
        assert!(!is_submit_input("\ra"));
        assert!(!is_submit_input(" \r"));
    }

    #[test]
    fn quit_intent_classification_rules() {
        let mut s = QuitIntentState::default();
        // Typed char-by-char: /quit + Enter.
        for c in ["/", "q", "u", "i", "t"] {
            assert_eq!(classify_input(&mut s, c), InputClass::Other);
        }
        assert_eq!(classify_input(&mut s, "\r"), InputClass::QuitIntent);

        // Pasted whole line.
        assert_eq!(classify_input(&mut s, "/exit\r"), InputClass::QuitIntent);

        // Control-key quits.
        assert_eq!(classify_input(&mut s, "\u{4}"), InputClass::QuitIntent);
        assert_eq!(classify_input(&mut s, "\u{3}"), InputClass::QuitIntent);

        // An ordinary prompt is a NonQuitSubmit at its Enter.
        assert_eq!(classify_input(&mut s, "fix the bug"), InputClass::Other);
        assert_eq!(classify_input(&mut s, "\r"), InputClass::NonQuitSubmit);

        // Backspace editing: "/quitX" + BS + Enter is a quit.
        assert_eq!(classify_input(&mut s, "/quitX"), InputClass::Other);
        assert_eq!(classify_input(&mut s, "\u{7f}"), InputClass::Other);
        assert_eq!(classify_input(&mut s, "\r"), InputClass::QuitIntent);

        // An escape sequence poisons the line until the next newline:
        // arrow-key navigation + Enter is NOT a detectable quit.
        assert_eq!(classify_input(&mut s, "/quit"), InputClass::Other);
        assert_eq!(classify_input(&mut s, "\u{1b}[A"), InputClass::Other);
        assert_eq!(classify_input(&mut s, "\r"), InputClass::NonQuitSubmit);

        // …and the poison clears after that newline.
        assert_eq!(classify_input(&mut s, "/quit\r"), InputClass::QuitIntent);
    }

    #[test]
    fn bracketed_paste_framing_rules() {
        // A16 validated: both agent TUIs enable DECSET 2004, so xterm.js
        // frames EVERY paste as \x1b[200~ + text + \x1b[201~ (with \n
        // normalized to \r). The framing is not user line content: it
        // must not poison, and the pasted text is literal.
        let mut s = QuitIntentState::default();

        // Pasted "/exit" then a real Enter (the exact wire shape:
        // one framed chunk, then a separate "\r") ⇒ QuitIntent.
        assert_eq!(
            classify_input(&mut s, "\u{1b}[200~/exit\u{1b}[201~"),
            InputClass::Other
        );
        assert_eq!(classify_input(&mut s, "\r"), InputClass::QuitIntent);

        // 0x03 INSIDE framing is literal pasted data, NOT a quit gesture.
        assert_eq!(
            classify_input(&mut s, "\u{1b}[200~ab\u{3}cd\u{1b}[201~"),
            InputClass::Other
        );
        assert_eq!(classify_input(&mut s, "\r"), InputClass::NonQuitSubmit);

        // A literal pasted newline clears the buffer (a multi-line blob
        // is not a submit); the last pasted line still evaluates at the
        // following REAL Enter.
        assert_eq!(
            classify_input(&mut s, "\u{1b}[200~echo hi\rls\u{1b}[201~"),
            InputClass::Other
        );
        assert_eq!(classify_input(&mut s, "\r"), InputClass::NonQuitSubmit);

        // Markers split across chunks (paste atomicity is observed, not
        // contractual — reconnect buffering can slice anywhere).
        assert_eq!(classify_input(&mut s, "\u{1b}[20"), InputClass::Other);
        assert_eq!(classify_input(&mut s, "0~/quit"), InputClass::Other);
        assert_eq!(classify_input(&mut s, "\u{1b}[201~"), InputClass::Other);
        assert_eq!(classify_input(&mut s, "\r"), InputClass::QuitIntent);
    }

    #[test]
    fn decrqm_reports_do_not_poison() {
        // A16.2: freshell's own client auto-answers DECRQM as synthetic
        // INPUT (\x1b[?<mode>;<status>$y, request-mode-bypass.ts:256).
        // Exact-grammar skip — a user typing /quit right after such a
        // reply (no intervening Enter) must still be detected.
        let mut s = QuitIntentState::default();
        assert_eq!(
            classify_input(&mut s, "\u{1b}[?2004;1$y"),
            InputClass::Other
        );
        assert_eq!(classify_input(&mut s, "/quit\r"), InputClass::QuitIntent);

        // Split across chunks too.
        assert_eq!(classify_input(&mut s, "\u{1b}[?20"), InputClass::Other);
        assert_eq!(
            classify_input(&mut s, "04;1$y/exit\r"),
            InputClass::QuitIntent
        );

        // A NEAR-miss (wrong grammar) still poisons like any other
        // escape sequence — the skip is exact, not a heuristic.
        assert_eq!(
            classify_input(&mut s, "\u{1b}[?2004;1$z"),
            InputClass::Other
        );
        assert_eq!(classify_input(&mut s, "/quit\r"), InputClass::NonQuitSubmit);
        assert_eq!(classify_input(&mut s, "/quit\r"), InputClass::QuitIntent);
    }

    #[test]
    fn extract_counts_a_bare_bel_and_strips_it() {
        let mut state = ParserState::new();
        let (cleaned, count) =
            extract_turn_complete_signals("hello\u{07}world", "claude", &mut state);
        assert_eq!(count, 1);
        assert_eq!(cleaned, "helloworld");
    }

    #[test]
    fn extract_ignores_bels_inside_osc_sequences() {
        let mut state = ParserState::new();
        // OSC 0;title BEL — a title-set sequence, its BEL is a terminator.
        let (cleaned, count) =
            extract_turn_complete_signals("\u{1b}]0;title\u{07}after", "claude", &mut state);
        assert_eq!(count, 0);
        assert_eq!(cleaned, "\u{1b}]0;title\u{07}after");
    }

    #[test]
    fn extract_tracks_osc_state_across_chunks() {
        let mut state = ParserState::new();
        let (_, count1) = extract_turn_complete_signals("\u{1b}]0;tit", "codex", &mut state);
        assert_eq!(count1, 0);
        assert!(state.in_osc);
        // The BEL that arrives in the NEXT chunk terminates the OSC — no signal.
        let (_, count2) = extract_turn_complete_signals("le\u{07}", "codex", &mut state);
        assert_eq!(count2, 0);
        assert!(!state.in_osc);
        // A bare BEL after that IS a signal.
        let (_, count3) = extract_turn_complete_signals("\u{07}", "codex", &mut state);
        assert_eq!(count3, 1);
    }

    #[test]
    fn extract_passes_through_for_non_signal_modes() {
        let mut state = ParserState::new();
        let (cleaned, count) = extract_turn_complete_signals("hi\u{07}", "shell", &mut state);
        assert_eq!(count, 0);
        assert_eq!(cleaned, "hi\u{07}");
    }

    #[test]
    fn tracker_count_accepts_leading_and_trailing_bels() {
        let state = ParserState::new();
        // Leading BEL (whitespace/controls before it are ignorable).
        assert_eq!(
            count_tracker_turn_complete_signals("\r\n\u{07}done", &state),
            1
        );
        // Trailing BEL (no visible output after).
        assert_eq!(
            count_tracker_turn_complete_signals("done\u{07} \r\n", &state),
            1
        );
    }

    #[test]
    fn tracker_count_rejects_a_sandwiched_bell() {
        let state = ParserState::new();
        // Visible output on BOTH sides: a stray sub-tool bell, not a signal.
        assert_eq!(
            count_tracker_turn_complete_signals("out\u{07}more-out", &state),
            0
        );
    }

    #[test]
    fn tracker_count_skips_escape_enclosed_bels() {
        let state = ParserState::new();
        assert_eq!(
            count_tracker_turn_complete_signals("\u{1b}]0;t\u{07}", &state),
            0
        );
        // CSI sequences don't eat a following bare BEL.
        assert_eq!(
            count_tracker_turn_complete_signals("\u{1b}[2K\u{07}", &state),
            1
        );
    }

    #[test]
    fn tracker_count_respects_carried_state() {
        let mut state = ParserState::new();
        let _ = extract_turn_complete_signals("\u{1b}]0;tit", "claude", &mut state);
        // Still inside the OSC from the previous chunk: this BEL terminates it.
        assert_eq!(count_tracker_turn_complete_signals("le\u{07}", &state), 0);
    }
}
