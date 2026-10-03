//! Human presentation via sartorial-core (https://github.com/matthewjameswatkins1978-cyber/Sartorial, MIT).
//!
//! Governing rule: **Terror Bat owns truth. Sartorial owns presentation.**
//!
//! ```text
//! Terror Bat Receipt → projection → sartorial_core::Document
//!   → Workwear + Terror Bat Theme → ResolvedStyle + Capabilities
//!   → TerminalRenderer (TTY) / PlainRenderer (pipes)
//! ```
//!
//! This module owns projection, theme, capability detection, and rendering.
//! It must never own: verdict calculation, oracle judgement, receipt
//! hashing or verification, evidence lookup, exit codes, or run state.
//!
//! The machine path (`--json`) never touches this module.

use anstyle::AnsiColor;
use sartorial_core::{
    Block, BorderStyle, Capabilities, ColorPolicy, Density, Document, Evidence as SEvidence, Fact,
    MarkdownRenderer, Notice, PlainRenderer, Preset, ResolvedStyle, Status, SymbolMode,
    TerminalRenderer, Theme,
};

use crate::adapters::AdapterInfo;
use crate::campaign::CampaignReceipt;
use crate::doctor::DoctorReport;
use crate::receipt::{Receipt, ReplayReport};
use crate::runner::{RunStatus, TERRORBAT_VERSION};

/// The canonical Terror Bat mark. ASCII-safe by design: it is the product
/// sigil, not a fallback, and is used identically on every human surface
/// whether or not Unicode is available.
pub const SIGIL: &str = "\\^v^/";

/// Application-owned theme: Terror Bat roles mapped to paint. Colours
/// express presentation roles only and never change meaning, ordering,
/// verdicts, statuses, or exit codes.
pub fn terrorbat_theme() -> Theme {
    Theme::builder("Terror Bat")
        .accent(AnsiColor::BrightRed)
        .heading(AnsiColor::Red)
        .success(AnsiColor::Green)
        .warning(AnsiColor::Yellow)
        .failure(AnsiColor::Red)
        .muted(AnsiColor::BrightBlack)
        .evidence(AnsiColor::BrightBlack)
        .code(AnsiColor::BrightWhite)
        .path(AnsiColor::BrightBlack)
        .number(AnsiColor::BrightWhite)
        .build()
}

/// Detect presentation capabilities using stdlib facilities only.
/// Conservative by design: colour needs an attended terminal without
/// NO_COLOR; Unicode is assumed only on the proven TTY path; hyperlinks,
/// motion and interactivity are always off. Width prefers COLUMNS when it
/// is a valid positive integer, else a stable fallback of 100.
pub fn detect_capabilities() -> Capabilities {
    use std::io::IsTerminal;
    let is_tty = std::io::stdout().is_terminal();
    let no_color = std::env::var_os("NO_COLOR").is_some();
    let width = std::env::var("COLUMNS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .filter(|&w| w > 0)
        .unwrap_or(100);
    Capabilities::explicit(
        width,
        is_tty,
        ColorPolicy::Auto,
        no_color,
        is_tty,
        false,
        false,
        false,
    )
}

/// Resolve Workwear + Terror Bat theme against explicit capabilities.
pub fn resolve_style(caps: &Capabilities) -> ResolvedStyle {
    let symbols = if caps.unicode {
        SymbolMode::Unicode
    } else {
        SymbolMode::Ascii
    };
    ResolvedStyle::resolve(
        Preset::Workwear,
        &terrorbat_theme(),
        Density::Compact,
        BorderStyle::Subtle,
        symbols,
        caps.color_enabled,
    )
}

/// Render a document: styled terminal output when attended, clean plain
/// text when redirected. No ANSI, no Unicode dependency, no motion.
pub fn render_document(doc: &Document, caps: &Capabilities) -> String {
    let style = resolve_style(caps);
    let rendered = if caps.is_tty {
        TerminalRenderer::render_to_string(doc, &style, caps)
    } else {
        PlainRenderer::render_to_string(doc, &style, caps)
    };
    rendered.expect("in-memory rendering cannot fail")
}

/// Internal Markdown projection (tests only; no public CLI surface yet).
/// Proves the same Document serves future GitHub-report rendering.
pub fn render_markdown(doc: &Document, caps: &Capabilities) -> String {
    let style = resolve_style(caps);
    MarkdownRenderer::render_to_string(doc, &style, caps).expect("in-memory rendering cannot fail")
}

/// Title grammar: symmetrical sigil framing on normal/wide terminals,
/// graceful single-sigil collapse on narrow ones. The product title is
/// never truncated to preserve symmetry.
///
/// The framed line MUST live in a case-preserving block (Details), never
/// in Title/Facts-names: Workwear's title casing uppercases display labels
/// and would mangle the canonical `\^v^/` mark into `\^V^/`.
pub fn title_text(base: &str, width: usize) -> String {
    if width >= 60 {
        format!("{SIGIL}  {base}  {SIGIL}")
    } else {
        format!("{SIGIL}  {base}")
    }
}

/// Sartorial Status is a *presentation* role for mechanical execution
/// state only. Verdicts and oracle results are never mapped into it —
/// they stay explicit labels/facts.
fn execution_face(status: RunStatus) -> Status {
    match status {
        RunStatus::Completed => Status::Ready,
        RunStatus::TimedOut => Status::Attention,
        RunStatus::Crashed => Status::Failed,
        RunStatus::Cancelled => Status::Skipped,
        RunStatus::Unsupported => Status::Skipped,
        RunStatus::PolicyDenied => Status::Attention,
        RunStatus::Invalid => Status::Failed,
        RunStatus::InfrastructureError => Status::Failed,
    }
}

fn short_sha(sha: &str) -> String {
    if sha.len() > 12 {
        sha[..12].to_string()
    } else {
        sha.to_string()
    }
}

fn wall_time(wall_ms: u64) -> String {
    if wall_ms >= 1000 {
        format!("{:.2} s", wall_ms as f64 / 1000.0)
    } else {
        format!("{wall_ms} ms")
    }
}

/// Project an authoritative receipt into a Sartorial Document. The three
/// truth layers — execution, oracle, verdict — stay visually distinct;
/// nothing is reinterpreted, only arranged.
pub fn document_receipt(r: &Receipt, caps: &Capabilities) -> Document {
    let mut doc = Document::new();
    // Sigil band first (case-preserving block), then the structured title
    // whose base name is already uppercase and therefore casing-immune.
    doc = doc.push(Block::Details {
        text: title_text("BAT RECEIPT", caps.width),
    });
    doc = doc.push(Block::Title {
        text: "BAT RECEIPT".to_string(),
        version: None,
    });
    doc = doc.push(Block::Subtitle {
        text: r.bat.id.clone(),
    });
    // The claim is a material fact: Facts values preserve case exactly.
    doc = doc.push(Block::Facts {
        facts: vec![Fact::new("Claim", r.claim_text.trim())],
    });

    // Layer 1: what mechanically happened.
    doc = doc.push(Block::StatusSection {
        label: "EXECUTION".to_string(),
        status: execution_face(r.execution.status),
    });
    let setup_total = r
        .execution
        .steps
        .iter()
        .filter(|s| s.phase == "setup")
        .count();
    let setup_done = r
        .execution
        .steps
        .iter()
        .filter(|s| s.phase == "setup" && s.status == RunStatus::Completed)
        .count();
    let run_total = r
        .execution
        .steps
        .iter()
        .filter(|s| s.phase == "run")
        .count();
    let run_done = r
        .execution
        .steps
        .iter()
        .filter(|s| s.phase == "run" && s.status == RunStatus::Completed)
        .count();
    doc = doc.push(Block::Facts {
        facts: vec![
            Fact::new("Status", format!("{:?}", r.execution.status)),
            Fact::new("Setup", format!("{setup_done}/{setup_total} completed")),
            Fact::new("Run", format!("{run_done}/{run_total} completed")),
            Fact::new("Wall time", wall_time(r.execution.wall_ms)),
        ],
    });

    // Layer 2: what the oracle determined. Facts first, then the
    // per-condition detail they summarise.
    let mut oracle_facts = Vec::new();
    let mut oracle_detail = String::new();
    match r.oracle.result {
        Some(result) => {
            oracle_facts.push(Fact::new("Result", result.label()));
            oracle_facts.push(Fact::new(
                "Conditions",
                r.oracle.conditions.len().to_string(),
            ));
            for c in &r.oracle.conditions {
                let note = c
                    .note
                    .as_ref()
                    .map(|n| format!(" - {n}"))
                    .unwrap_or_default();
                oracle_detail.push_str(&format!("- {} -> {:?}{note}\n", c.condition, c.result));
            }
        }
        None => {
            let reason = r
                .oracle
                .note
                .as_deref()
                .unwrap_or("run did not complete; oracle not evaluated");
            oracle_facts.push(Fact::new("Result", "NOT EVALUATED"));
            oracle_facts.push(Fact::new("Reason", reason));
        }
    }
    doc = doc.push(Block::Facts {
        facts: oracle_facts,
    });
    if !oracle_detail.is_empty() {
        doc = doc.push(Block::Details {
            text: oracle_detail.trim_end().to_string(),
        });
    }

    // Layer 3: what may responsibly be claimed. The badge carries visual
    // weight through the accent role — never success-green: PROVEN is a
    // falsification finding, not a celebration.
    doc = doc.push(Block::BadgeSection {
        label: "VERDICT".to_string(),
        badge: r.verdict.as_str().to_string(),
    });
    doc = doc.push(Block::Details {
        text: r.verdict_meaning.clone(),
    });

    doc = doc.push(Block::Facts {
        facts: vec![
            Fact::new("Repository", r.target.root.clone()),
            Fact::new("Commit", short_sha(&r.target.commit)),
        ],
    });

    let mut evidence_items = Vec::new();
    for item in &r.evidence {
        let summary = match item.kind.as_str() {
            "git_diff" => "Git diff".to_string(),
            "git_status" => "Git status".to_string(),
            "base_snapshot" => "Base snapshot".to_string(),
            other => other.to_string(),
        };
        // Handles are material facts: the compact Evidence rendering shows
        // the summary, so the reference rides in the summary text itself.
        // This keeps every renderer (terminal, plain, Markdown) honest.
        let summary = format!("{summary}  {}", item.reference.0);
        let mut ev = SEvidence::new(summary).with_handle(item.reference.0.clone());
        if item.truncated {
            ev = ev.truncated();
        }
        evidence_items.push(ev);
    }
    doc = doc.push(Block::BadgeSection {
        label: "EVIDENCE".to_string(),
        badge: format!("{} objects", evidence_items.len()),
    });
    doc = doc.push(Block::Evidence {
        items: evidence_items,
    });

    doc = doc.push(Block::Facts {
        facts: vec![Fact::new("Worktree mode", r.isolation.mode.clone())],
    });
    doc = doc.push(Block::List {
        ordered: false,
        items: r.isolation.guarantees.clone(),
    });
    doc = doc.push(Block::List {
        ordered: false,
        items: r.isolation.non_guarantees.clone(),
    });

    doc = doc.push(Block::Facts {
        facts: vec![Fact::new(
            "Status",
            match r.cleanup.status {
                crate::runner::CleanupStatus::Succeeded => "SUCCEEDED".to_string(),
                crate::runner::CleanupStatus::NotAttempted => "NOT ATTEMPTED".to_string(),
                crate::runner::CleanupStatus::Failed => "FAILED".to_string(),
            },
        )],
    });
    if matches!(r.cleanup.status, crate::runner::CleanupStatus::Failed) {
        let mut detail = String::new();
        if let Some(path) = &r.cleanup.path {
            detail.push_str(&format!("Leftover worktree:\n  {path}\n"));
        }
        if let Some(error) = &r.cleanup.error {
            detail.push_str(error);
        }
        doc = doc.push(Block::Notices {
            notices: vec![Notice::error("Cleanup FAILED").with_detail(detail.trim().to_string())],
        });
    }

    for limitation in &r.limitations {
        doc = doc.push(Block::Notices {
            notices: vec![Notice::warning(limitation.clone())],
        });
    }

    doc = doc.push(Block::Facts {
        facts: vec![
            Fact::new("Receipt", r.receipt_id.clone()),
            Fact::new("Execution", r.execution_id.clone()),
            Fact::new("Replay", r.reproduction.replay_command.clone()),
        ],
    });

    doc
}

/// Project a doctor report. The DoctorReport stays authoritative; this
/// arranges its checks into titled sections.
pub fn document_doctor(rep: &DoctorReport, caps: &Capabilities) -> Document {
    let mut doc = Document::new();
    doc = doc.push(Block::Details {
        text: title_text("TERROR BAT DOCTOR", caps.width),
    });
    doc = doc.push(Block::Title {
        text: "TERROR BAT DOCTOR".to_string(),
        version: Some(TERRORBAT_VERSION.to_string()),
    });

    let mut core_facts = Vec::new();
    for c in &rep.core {
        core_facts.push(Fact::new(
            c.name.clone(),
            format!("{} {}", if c.ok { "OK" } else { "FAIL" }, c.detail),
        ));
    }
    doc = doc.push(Block::BadgeSection {
        label: "CORE".to_string(),
        badge: format!(
            "{}/{} checks",
            rep.core.iter().filter(|c| c.ok).count(),
            rep.core.len()
        ),
    });
    doc = doc.push(Block::Facts { facts: core_facts });

    let mut optional_facts = Vec::new();
    for c in &rep.optional_tools {
        optional_facts.push(Fact::new(c.name.clone(), c.detail.clone()));
    }
    doc = doc.push(Block::BadgeSection {
        label: "OPTIONAL".to_string(),
        badge: format!("{} tools", optional_facts.len()),
    });
    doc = doc.push(Block::Facts {
        facts: optional_facts,
    });

    let mut isolation_facts = Vec::new();
    for c in &rep.isolation {
        isolation_facts.push(Fact::new(c.name.clone(), c.detail.clone()));
    }
    doc = doc.push(Block::BadgeSection {
        label: "ISOLATION".to_string(),
        badge: "stated, not enforced".to_string(),
    });
    doc = doc.push(Block::Facts {
        facts: isolation_facts,
    });

    doc = doc.push(Block::StatusSection {
        label: "OVERALL".to_string(),
        status: if rep.overall_ready {
            Status::Ready
        } else {
            Status::Failed
        },
    });
    doc
}

/// Project a replay comparison. Same-verdict is reported as same-verdict —
/// never as an identical execution.
pub fn document_replay(report: &ReplayReport, caps: &Capabilities) -> Document {
    let mut doc = Document::new();
    doc = doc.push(Block::Details {
        text: title_text("TERROR BAT REPLAY", caps.width),
    });
    doc = doc.push(Block::Title {
        text: "TERROR BAT REPLAY".to_string(),
        version: None,
    });
    let yn = |same: bool| {
        if same {
            "same".to_string()
        } else {
            "DIFFERENT".to_string()
        }
    };
    doc = doc.push(Block::Facts {
        facts: vec![
            Fact::new("Original execution", report.original_execution_id.clone()),
            Fact::new("New execution", report.new_execution_id.clone()),
            Fact::new("Original receipt", report.original_receipt_id.clone()),
            Fact::new("New receipt", report.new_receipt_id.clone()),
            Fact::new("Execution status", yn(report.same_status)),
            Fact::new("Oracle result", yn(report.same_oracle)),
            Fact::new("Verdict", yn(report.same_verdict)),
        ],
    });
    if !report.evidence_changes.is_empty() {
        doc = doc.push(Block::List {
            ordered: false,
            items: report.evidence_changes.clone(),
        });
    }
    if !report.environment_changes.is_empty() {
        doc = doc.push(Block::List {
            ordered: false,
            items: report.environment_changes.clone(),
        });
    }
    for note in &report.notes {
        doc = doc.push(Block::Notices {
            notices: vec![Notice::info(note.clone())],
        });
    }
    doc = doc.push(Block::Details {
        text: "A shared verdict alone does not make two executions equivalent; \
               compare the evidence identities above."
            .to_string(),
    });
    doc
}

/// Project a campaign aggregate into a Sartorial Document. Verdict labels
/// are rendered verbatim — NOT OBSERVED is never reworded as
/// PASSED/SAFE/CORRECT. The FIRST PROVEN receipt reference is surfaced so a
/// falsification finding is one lookup away from its evidence.
pub fn document_campaign(c: &CampaignReceipt, caps: &Capabilities) -> Document {
    let mut doc = Document::new();
    // Sigil band first (case-preserving block), matching the receipt shape.
    doc = doc.push(Block::Details {
        text: title_text("BAT CAMPAIGN", caps.width),
    });
    doc = doc.push(Block::Title {
        text: "BAT CAMPAIGN".to_string(),
        version: None,
    });
    doc = doc.push(Block::Subtitle {
        text: c.pack_id.clone(),
    });
    doc = doc.push(Block::Facts {
        facts: vec![
            Fact::new("Campaign", c.campaign_id.clone()),
            Fact::new("Pack", c.pack.clone()),
            Fact::new("Target commit", short_sha(&c.target_commit)),
            Fact::new("Requested runs", c.requested_runs.to_string()),
            Fact::new("Completed runs", c.completed_runs.to_string()),
            Fact::new("Children", c.children.len().to_string()),
            Fact::new(
                "Stopped early",
                if c.stopped_early { "yes" } else { "no" }.to_string(),
            ),
        ],
    });

    doc = doc.push(Block::BadgeSection {
        label: "VERDICTS".to_string(),
        badge: format!("{} children", c.children.len()),
    });
    doc = doc.push(Block::Facts {
        facts: crate::campaign::VERDICT_LABELS
            .iter()
            .map(|label| {
                Fact::new(
                    (*label).to_string(),
                    c.summary.get(*label).copied().unwrap_or(0).to_string(),
                )
            })
            .collect(),
    });

    if let Some(first) = c
        .children
        .iter()
        .find(|ch| ch.verdict == crate::receipt::Verdict::Proven)
    {
        doc = doc.push(Block::Facts {
            facts: vec![
                Fact::new("First PROVEN receipt", first.receipt_id.clone()),
                Fact::new("First PROVEN execution", first.execution_id.clone()),
            ],
        });
    }

    let items: Vec<String> = c
        .children
        .iter()
        .map(|ch| {
            format!(
                "iteration {} \u{00b7} entry {} \u{00b7} {} \u{00b7} {} \u{00b7} {}",
                ch.iteration,
                ch.entry_index,
                ch.bat_id,
                ch.verdict.as_str(),
                ch.receipt_id,
            )
        })
        .collect();
    if !items.is_empty() {
        doc = doc.push(Block::List {
            ordered: false,
            items,
        });
    }

    if c.stopped_early
        && let Some(reason) = &c.stop_reason
    {
        doc = doc.push(Block::Notices {
            notices: vec![Notice::info(reason.clone())],
        });
    }

    doc = doc.push(Block::Details {
        text: "Every child is an ordinary first-class run; later results never overwrite \
               earlier ones."
            .to_string(),
    });
    doc = doc.push(Block::Facts {
        facts: vec![
            Fact::new("Pack", c.pack.clone()),
            Fact::new("Campaign", c.campaign_id.clone()),
        ],
    });
    doc
}

/// Convenience: project and render a campaign for the detected context.
pub fn render_campaign(c: &CampaignReceipt, caps: &Capabilities) -> String {
    render_document(&document_campaign(c, caps), caps)
}

/// Convenience: project and render a receipt for the detected context.
pub fn render_receipt(r: &Receipt, caps: &Capabilities) -> String {
    render_document(&document_receipt(r, caps), caps)
}

/// Convenience: project and render a doctor report.
pub fn render_doctor(rep: &DoctorReport, caps: &Capabilities) -> String {
    render_document(&document_doctor(rep, caps), caps)
}

/// Convenience: project and render a replay comparison followed by the new
/// receipt view (same order the CLI has always printed).
/// Project the universal adapter catalogue. Agents discover adapters here,
/// not from prompt folklore: status values are builtin-stable,
/// builtin-partial, composition, external-protocol, or planned.
pub fn document_adapters(adapters: &[AdapterInfo], caps: &Capabilities) -> Document {
    let mut doc = Document::new();
    doc = doc.push(Block::Details {
        text: title_text("UNIVERSAL ADAPTERS", caps.width),
    });
    doc = doc.push(Block::Title {
        text: "UNIVERSAL ADAPTERS".to_string(),
        version: None,
    });
    doc = doc.push(Block::Subtitle {
        text: "Can one of the universal adapters already reach this thing?".to_string(),
    });
    for a in adapters {
        doc = doc.push(Block::Facts {
            facts: vec![
                Fact::new("Adapter", format!("{} — {}", a.name, a.title)),
                Fact::new("Status", a.status.clone()),
                Fact::new("Description", a.description.clone()),
            ],
        });
        let ops: Vec<String> = a
            .operations
            .iter()
            .map(|o| {
                format!(
                    "{}.{} — {} (requires {})",
                    a.name, o.action, o.description, o.capability
                )
            })
            .collect();
        if !ops.is_empty() {
            doc = doc.push(Block::List {
                ordered: false,
                items: ops,
            });
        }
    }
    doc
}

/// Project one adapter's full detail: operations, inputs, outputs,
/// capabilities, constraints, examples, and platform limitations.
pub fn document_adapter_inspect(a: &AdapterInfo, caps: &Capabilities) -> Document {
    let mut doc = Document::new();
    doc = doc.push(Block::Details {
        text: title_text("ADAPTER", caps.width),
    });
    doc = doc.push(Block::Title {
        text: "ADAPTER".to_string(),
        version: None,
    });
    doc = doc.push(Block::Subtitle {
        text: format!("{} — {}", a.name, a.title),
    });
    doc = doc.push(Block::Facts {
        facts: vec![
            Fact::new("Status", a.status.clone()),
            Fact::new("Description", a.description.clone()),
        ],
    });
    for o in &a.operations {
        doc = doc.push(Block::Facts {
            facts: vec![
                Fact::new("Operation", format!("{}.{}", a.name, o.action)),
                Fact::new("Description", o.description.clone()),
                Fact::new("Inputs", o.inputs.join("; ")),
                Fact::new("Outputs", o.outputs.join("; ")),
                Fact::new("Requires", o.capability.clone()),
                Fact::new("Example", o.example.clone()),
            ],
        });
    }
    doc = doc.push(Block::Facts {
        facts: vec![
            Fact::new("Capabilities", a.capabilities.join(", ")),
            Fact::new("Constraints", a.constraints.join(" ")),
        ],
    });
    if !a.examples.is_empty() {
        doc = doc.push(Block::List {
            ordered: false,
            items: a.examples.clone(),
        });
    }
    if !a.platform_notes.is_empty() {
        doc = doc.push(Block::Notices {
            notices: a
                .platform_notes
                .iter()
                .map(|n| Notice::info(n.clone()))
                .collect(),
        });
    }
    doc
}

/// Convenience: project and render the adapter catalogue.
pub fn render_adapters(adapters: &[AdapterInfo], caps: &Capabilities) -> String {
    render_document(&document_adapters(adapters, caps), caps)
}

/// Convenience: project and render one adapter's detail.
pub fn render_adapter_inspect(a: &AdapterInfo, caps: &Capabilities) -> String {
    render_document(&document_adapter_inspect(a, caps), caps)
}

pub fn render_replay(
    report: &crate::receipt::ReplayReport,
    new_receipt: &Receipt,
    caps: &Capabilities,
) -> String {
    let mut out = render_document(&document_replay(report, caps), caps);
    out.push_str(&render_receipt(new_receipt, caps));
    out
}
