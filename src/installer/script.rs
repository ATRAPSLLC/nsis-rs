//! Script-level control-flow analysis for NSIS bytecode.
//!
//! This module builds a conservative control-flow model from raw NSIS entries:
//! roots, functions, basic blocks, and edges. Dynamic jumps and calls are
//! represented explicitly instead of being dropped.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::{
    error::Error,
    installer::{Callback, NsisInstaller},
    nsis::Entry,
    opcode,
};

/// Script-level control-flow analysis for an installer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptAnalysis {
    /// Number of entries in the script.
    pub entry_count: usize,
    /// Discovered script roots, such as sections, callbacks, pages, and calls.
    pub roots: Vec<ScriptRoot>,
    /// Discovered function-like regions.
    pub functions: Vec<ScriptFunction>,
    /// Basic blocks split at roots, branch targets, and terminators.
    pub blocks: Vec<BasicBlock>,
    /// Control-flow edges between blocks or special targets.
    pub edges: Vec<ControlFlowEdge>,
    /// Mapping from entry index to containing basic-block id.
    pub entry_to_block: Vec<Option<usize>>,
    /// Non-fatal analysis diagnostics.
    pub diagnostics: Vec<ScriptAnalysisDiagnostic>,
}

impl ScriptAnalysis {
    /// Returns the basic block containing an entry index.
    ///
    /// # Arguments
    ///
    /// * `entry` - Zero-based script entry index.
    ///
    /// # Returns
    ///
    /// The containing [`BasicBlock`], or `None` if `entry` is outside the
    /// script or the entry was not assigned to a block.
    pub fn block_for_entry(&self, entry: usize) -> Option<&BasicBlock> {
        self.entry_to_block
            .get(entry)
            .copied()
            .flatten()
            .and_then(|block| self.blocks.get(block))
    }

    /// Returns the function containing an entry index.
    ///
    /// # Arguments
    ///
    /// * `entry` - Zero-based script entry index.
    ///
    /// # Returns
    ///
    /// The containing [`ScriptFunction`], or `None` if the entry is outside the
    /// script, unreachable, or assigned to no discovered function.
    pub fn function_for_entry(&self, entry: usize) -> Option<&ScriptFunction> {
        let block = self.block_for_entry(entry)?;
        let function = block.function?;
        self.functions.get(function)
    }

    /// Returns all roots starting at an entry index.
    ///
    /// # Arguments
    ///
    /// * `entry` - Zero-based script entry index.
    ///
    /// # Returns
    ///
    /// An iterator over every [`ScriptRoot`] that starts at `entry`. Multiple
    /// roots can share one entry, for example when a page handler and callback
    /// point to the same bytecode.
    pub fn roots_for_entry(&self, entry: usize) -> impl Iterator<Item = &ScriptRoot> {
        self.roots.iter().filter(move |root| root.entry == entry)
    }

    /// Returns outgoing edges from a basic block.
    ///
    /// # Arguments
    ///
    /// * `block` - Basic-block id.
    ///
    /// # Returns
    ///
    /// An iterator over all [`ControlFlowEdge`] values whose source block is
    /// `block`.
    pub fn outgoing_edges(&self, block: usize) -> impl Iterator<Item = &ControlFlowEdge> {
        self.edges
            .iter()
            .filter(move |edge| edge.source_block == block)
    }

    /// Returns incoming edges to a basic block.
    ///
    /// # Arguments
    ///
    /// * `block` - Basic-block id.
    ///
    /// # Returns
    ///
    /// An iterator over all [`ControlFlowEdge`] values whose static target
    /// block is `block`. Dynamic, invalid, absent, and exit targets are not
    /// returned by this helper.
    pub fn incoming_edges(&self, block: usize) -> impl Iterator<Item = &ControlFlowEdge> {
        self.edges
            .iter()
            .filter(move |edge| edge.target_block == Some(block))
    }

    /// Returns outgoing edges from the basic block containing an entry.
    ///
    /// # Arguments
    ///
    /// * `entry` - Zero-based script entry index.
    ///
    /// # Returns
    ///
    /// An iterator over all outgoing edges for the entry's containing block. If
    /// the entry is outside the script, the iterator is empty.
    pub fn outgoing_edges_from_entry(
        &self,
        entry: usize,
    ) -> impl Iterator<Item = &ControlFlowEdge> {
        let block = self.block_for_entry(entry).map(|block| block.id);
        self.edges
            .iter()
            .filter(move |edge| Some(edge.source_block) == block)
    }

    /// Returns the best known symbol for an entry index.
    ///
    /// Function roots are preferred over plain labels. Returns `None` if the
    /// entry has no known symbolic name.
    ///
    /// # Arguments
    ///
    /// * `entry` - Zero-based script entry index.
    ///
    /// # Returns
    ///
    /// A borrowed symbol name such as `".onInit"`, `"section_0"`, or
    /// `"label_42"`.
    pub fn symbol_for_entry(&self, entry: usize) -> Option<&str> {
        self.functions
            .iter()
            .find(|func| func.entry == entry)
            .map(|func| func.name.as_str())
            .or_else(|| {
                self.roots
                    .iter()
                    .find(|root| root.entry == entry)
                    .map(|root| root.name.as_str())
            })
    }
}

/// A known script entry root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptRoot {
    /// Entry index where the root starts.
    pub entry: usize,
    /// Root kind.
    pub kind: ScriptRootKind,
    /// Stable display name for the root.
    pub name: String,
}

/// The source of a script root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptRootKind {
    /// Installer section body.
    Section {
        /// Section index.
        index: usize,
    },
    /// Common-header callback.
    Callback {
        /// Callback slot.
        callback: Callback,
    },
    /// Page callback.
    Page {
        /// Page index.
        index: usize,
        /// Page handler kind.
        handler: PageHandler,
    },
    /// Target of a static `EW_CALL`.
    CalledFunction,
    /// Target of a static `Call :label`.
    CalledLabel,
    /// Target of a static jump or branch.
    Label,
    /// Entry zero fallback when no other root exists.
    EntryZero,
}

/// Page callback slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageHandler {
    /// Page pre-creation callback.
    Pre,
    /// Page show callback.
    Show,
    /// Page leave callback.
    Leave,
}

/// A function-like region in the NSIS script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptFunction {
    /// Function id, indexing [`ScriptAnalysis::functions`].
    pub id: usize,
    /// Primary function name.
    pub name: String,
    /// Entry index where the function starts.
    pub entry: usize,
    /// Exclusive end entry index, if the function has at least one block.
    pub end: Option<usize>,
    /// Root ids associated with this function.
    pub roots: Vec<usize>,
    /// Basic-block ids reached inside this function.
    pub blocks: Vec<usize>,
}

/// A basic block of consecutive entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BasicBlock {
    /// Block id, indexing [`ScriptAnalysis::blocks`].
    pub id: usize,
    /// Function id if this block belongs to a discovered function.
    pub function: Option<usize>,
    /// First entry index in the block.
    pub start: usize,
    /// Exclusive end entry index.
    pub end: usize,
}

/// A control-flow edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlFlowEdge {
    /// Source block id.
    pub source_block: usize,
    /// Source entry index.
    pub source_entry: usize,
    /// Edge kind.
    pub kind: EdgeKind,
    /// Target block id if the target is a known entry.
    pub target_block: Option<usize>,
    /// Edge target.
    pub target: ControlFlowTarget,
}

/// The reason for a control-flow edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EdgeKind {
    /// Ordinary fallthrough to the next entry.
    Fallthrough,
    /// Unconditional jump.
    Jump,
    /// Conditional branch.
    Branch {
        /// Branch condition represented by this edge.
        condition: BranchCondition,
    },
    /// Static or dynamic call edge.
    Call {
        /// Whether this is a `Call :label` form.
        label_style: bool,
    },
    /// Return from a function-like region.
    Return,
    /// Abort command.
    Abort,
    /// Quit command.
    Quit,
}

/// A conditional branch condition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BranchCondition {
    /// `IfFileExists` true/file-present branch.
    IfFileExists,
    /// `IfFileExists` false/file-missing branch.
    IfFileMissing,
    /// `IfFlag` true/set branch.
    IfFlagSet,
    /// `IfFlag` false/clear branch.
    IfFlagClear,
    /// `MessageBox` first button branch.
    MessageBoxButton1 {
        /// Button id stored in the entry.
        button: i32,
    },
    /// `MessageBox` second button branch.
    MessageBoxButton2 {
        /// Button id stored in the entry.
        button: i32,
    },
    /// `StrCmp` equal branch.
    StrEqual,
    /// `StrCmp` not-equal branch.
    StrNotEqual,
    /// `IntCmp` equal branch.
    IntEqual,
    /// `IntCmp` less-than branch.
    IntLess,
    /// `IntCmp` greater-than branch.
    IntGreater,
    /// `IsWindow` true branch.
    IsWindow,
    /// `IsWindow` false branch.
    IsNotWindow,
}

/// A resolved control-flow target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlFlowTarget {
    /// A statically known entry index.
    Entry(usize),
    /// A target computed from an NSIS variable.
    DynamicVariable(u16),
    /// Process/script exit.
    Exit,
    /// Opcode-specific absent target, usually encoded as zero.
    None,
    /// A malformed static target.
    Invalid(i32),
}

/// Non-fatal diagnostic emitted during script analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptAnalysisDiagnostic {
    /// A target operand referenced an entry outside the script.
    InvalidTarget {
        /// Entry containing the invalid target.
        entry: usize,
        /// Operand index containing the invalid target.
        operand: usize,
        /// Raw target value.
        raw: i32,
    },
}

#[derive(Debug, Clone)]
struct PendingEdge {
    source_entry: usize,
    operand: Option<usize>,
    kind: EdgeKind,
    target: ControlFlowTarget,
}

/// Builds script-level control-flow analysis.
///
/// # Arguments
///
/// * `installer` - Parsed installer whose script should be analyzed.
///
/// # Returns
///
/// A [`ScriptAnalysis`] with roots, functions, basic blocks, edges, block
/// lookup tables, and non-fatal diagnostics.
///
/// # Errors
///
/// Returns [`Error`] if any entry, section, or page fails to parse while
/// constructing the graph.
pub fn analyze(installer: &NsisInstaller<'_>) -> Result<ScriptAnalysis, Error> {
    let entries = collect_entries(installer)?;
    let entry_count = entries.len();
    let mut roots = collect_structural_roots(installer, entry_count)?;
    let mut edges = Vec::new();
    let mut diagnostics = Vec::new();
    let mut leaders = BTreeSet::new();

    if entry_count > 0 {
        leaders.insert(0);
        roots.push(ScriptRoot {
            entry: 0,
            kind: ScriptRootKind::EntryZero,
            name: "entry_0".to_string(),
        });
    }

    for root in &roots {
        leaders.insert(root.entry);
    }

    for (index, entry) in entries.iter().enumerate() {
        let flow = classify_entry(installer, entry, index, entry_count);
        if flow.starts_next_block
            && let Some(next) = index.checked_add(1)
            && next < entry_count
        {
            leaders.insert(next);
        }
        for edge in flow.edges {
            if let ControlFlowTarget::Entry(target) = edge.target {
                leaders.insert(target);
                if edge.kind == EdgeKind::Jump || matches!(edge.kind, EdgeKind::Branch { .. }) {
                    add_root(&mut roots, target, ScriptRootKind::Label);
                }
                if let EdgeKind::Call { label_style } = edge.kind {
                    if label_style {
                        add_root(&mut roots, target, ScriptRootKind::CalledLabel);
                    } else {
                        add_root(&mut roots, target, ScriptRootKind::CalledFunction);
                    }
                }
            }
            if let ControlFlowTarget::Invalid(raw) = edge.target {
                diagnostics.push(ScriptAnalysisDiagnostic::InvalidTarget {
                    entry: index,
                    operand: edge.operand.unwrap_or(0),
                    raw,
                });
            }
            edges.push(edge);
        }
    }

    for root in &roots {
        leaders.insert(root.entry);
    }

    let (mut blocks, entry_to_block) = build_blocks(entry_count, &leaders);
    let control_edges = materialize_edges(edges, &entry_to_block);
    let mut functions = build_functions(&roots, &blocks, &control_edges);
    assign_block_functions(&mut blocks, &mut functions);

    Ok(ScriptAnalysis {
        entry_count,
        roots,
        functions,
        blocks,
        edges: control_edges,
        entry_to_block,
        diagnostics,
    })
}

fn collect_entries<'a>(installer: &'a NsisInstaller<'a>) -> Result<Vec<Entry<'a>>, Error> {
    let mut entries = Vec::new();
    for entry in installer.entries() {
        entries.push(entry?);
    }
    Ok(entries)
}

fn collect_structural_roots(
    installer: &NsisInstaller<'_>,
    entry_count: usize,
) -> Result<Vec<ScriptRoot>, Error> {
    let mut roots = Vec::new();

    for (index, section) in installer.sections().enumerate() {
        let section = section?;
        if let Some(entry) = direct_index(section.code(), entry_count) {
            roots.push(ScriptRoot {
                entry,
                kind: ScriptRootKind::Section { index },
                name: format!("section_{index}"),
            });
        }
    }

    for callback in Callback::ALL {
        if let Some(entry) = installer.callback(callback)
            && entry < entry_count
        {
            roots.push(ScriptRoot {
                entry,
                kind: ScriptRootKind::Callback { callback },
                name: callback.name().to_string(),
            });
        }
    }

    for (index, page) in installer.pages().enumerate() {
        let page = page?;
        add_page_root(
            &mut roots,
            entry_count,
            index,
            PageHandler::Pre,
            page.prefunc(),
        );
        add_page_root(
            &mut roots,
            entry_count,
            index,
            PageHandler::Show,
            page.showfunc(),
        );
        add_page_root(
            &mut roots,
            entry_count,
            index,
            PageHandler::Leave,
            page.leavefunc(),
        );
    }

    Ok(roots)
}

fn add_page_root(
    roots: &mut Vec<ScriptRoot>,
    entry_count: usize,
    index: usize,
    handler: PageHandler,
    raw: i32,
) {
    if let Some(entry) = direct_index(raw, entry_count) {
        let suffix = match handler {
            PageHandler::Pre => "pre",
            PageHandler::Show => "show",
            PageHandler::Leave => "leave",
        };
        roots.push(ScriptRoot {
            entry,
            kind: ScriptRootKind::Page { index, handler },
            name: format!("page_{index}_{suffix}"),
        });
    }
}

#[derive(Debug, Clone)]
struct EntryFlow {
    edges: Vec<PendingEdge>,
    starts_next_block: bool,
}

fn classify_entry(
    installer: &NsisInstaller<'_>,
    entry: &Entry<'_>,
    index: usize,
    entry_count: usize,
) -> EntryFlow {
    let opcode = installer.normalize_opcode(entry.which());
    classify_opcode(opcode, entry, index, entry_count)
}

fn classify_opcode(opcode: i32, entry: &Entry<'_>, index: usize, entry_count: usize) -> EntryFlow {
    let mut edges = Vec::new();
    let mut starts_next_block = false;

    match opcode {
        opcode::EW_RET => {
            edges.push(PendingEdge {
                source_entry: index,
                operand: None,
                kind: EdgeKind::Return,
                target: ControlFlowTarget::Exit,
            });
            starts_next_block = true;
        }
        opcode::EW_ABORT => {
            edges.push(PendingEdge {
                source_entry: index,
                operand: None,
                kind: EdgeKind::Abort,
                target: ControlFlowTarget::Exit,
            });
            starts_next_block = true;
        }
        opcode::EW_QUIT => {
            edges.push(PendingEdge {
                source_entry: index,
                operand: None,
                kind: EdgeKind::Quit,
                target: ControlFlowTarget::Exit,
            });
            starts_next_block = true;
        }
        opcode::EW_NOP => {
            if entry.offset(0) == 0 {
                add_fallthrough(&mut edges, index, entry_count);
            } else {
                edges.push(PendingEdge {
                    source_entry: index,
                    operand: Some(0),
                    kind: EdgeKind::Jump,
                    target: resolve_target(entry.offset(0), entry_count),
                });
                starts_next_block = true;
            }
        }
        opcode::EW_CALL => {
            let label_style = entry.offset(1) == 1;
            edges.push(PendingEdge {
                source_entry: index,
                operand: Some(0),
                kind: EdgeKind::Call { label_style },
                target: resolve_call_target(entry.offset(0), entry_count),
            });
            add_fallthrough(&mut edges, index, entry_count);
            starts_next_block = true;
        }
        opcode::EW_IFFILEEXISTS => {
            add_branch(
                &mut edges,
                index,
                entry_count,
                BranchCondition::IfFileExists,
                1,
                entry.offset(1),
            );
            add_branch(
                &mut edges,
                index,
                entry_count,
                BranchCondition::IfFileMissing,
                2,
                entry.offset(2),
            );
            starts_next_block = true;
        }
        opcode::EW_IFFLAG => {
            add_branch(
                &mut edges,
                index,
                entry_count,
                BranchCondition::IfFlagSet,
                0,
                entry.offset(0),
            );
            add_branch(
                &mut edges,
                index,
                entry_count,
                BranchCondition::IfFlagClear,
                1,
                entry.offset(1),
            );
            starts_next_block = true;
        }
        opcode::EW_MESSAGEBOX => {
            if entry.offset(2) != 0 || entry.offset(3) != 0 {
                add_branch(
                    &mut edges,
                    index,
                    entry_count,
                    BranchCondition::MessageBoxButton1 {
                        button: entry.offset(2),
                    },
                    3,
                    entry.offset(3),
                );
            }
            if entry.offset(4) != 0 || entry.offset(5) != 0 {
                add_branch(
                    &mut edges,
                    index,
                    entry_count,
                    BranchCondition::MessageBoxButton2 {
                        button: entry.offset(4),
                    },
                    5,
                    entry.offset(5),
                );
            }
            add_fallthrough(&mut edges, index, entry_count);
            starts_next_block = true;
        }
        opcode::EW_STRCMP => {
            add_branch(
                &mut edges,
                index,
                entry_count,
                BranchCondition::StrEqual,
                2,
                entry.offset(2),
            );
            add_branch(
                &mut edges,
                index,
                entry_count,
                BranchCondition::StrNotEqual,
                3,
                entry.offset(3),
            );
            starts_next_block = true;
        }
        opcode::EW_INTCMP => {
            add_branch(
                &mut edges,
                index,
                entry_count,
                BranchCondition::IntEqual,
                2,
                entry.offset(2),
            );
            add_branch(
                &mut edges,
                index,
                entry_count,
                BranchCondition::IntLess,
                3,
                entry.offset(3),
            );
            add_branch(
                &mut edges,
                index,
                entry_count,
                BranchCondition::IntGreater,
                4,
                entry.offset(4),
            );
            starts_next_block = true;
        }
        opcode::EW_ISWINDOW => {
            add_branch(
                &mut edges,
                index,
                entry_count,
                BranchCondition::IsWindow,
                1,
                entry.offset(1),
            );
            add_branch(
                &mut edges,
                index,
                entry_count,
                BranchCondition::IsNotWindow,
                2,
                entry.offset(2),
            );
            starts_next_block = true;
        }
        _ => {
            add_fallthrough(&mut edges, index, entry_count);
        }
    }

    EntryFlow {
        edges,
        starts_next_block,
    }
}

fn add_fallthrough(edges: &mut Vec<PendingEdge>, index: usize, entry_count: usize) {
    let Some(next) = index.checked_add(1) else {
        return;
    };
    if next < entry_count {
        edges.push(PendingEdge {
            source_entry: index,
            operand: None,
            kind: EdgeKind::Fallthrough,
            target: ControlFlowTarget::Entry(next),
        });
    }
}

fn add_branch(
    edges: &mut Vec<PendingEdge>,
    index: usize,
    entry_count: usize,
    condition: BranchCondition,
    operand: usize,
    raw: i32,
) {
    let target = if raw == 0 {
        index
            .checked_add(1)
            .filter(|next| *next < entry_count)
            .map(ControlFlowTarget::Entry)
            .unwrap_or(ControlFlowTarget::Exit)
    } else {
        resolve_target(raw, entry_count)
    };
    edges.push(PendingEdge {
        source_entry: index,
        operand: Some(operand),
        kind: EdgeKind::Branch { condition },
        target,
    });
}

fn resolve_call_target(raw: i32, entry_count: usize) -> ControlFlowTarget {
    if raw == 0 {
        ControlFlowTarget::None
    } else {
        resolve_target(raw, entry_count)
    }
}

fn resolve_target(raw: i32, entry_count: usize) -> ControlFlowTarget {
    if raw > 0 {
        return positive_index(raw, entry_count)
            .map(ControlFlowTarget::Entry)
            .unwrap_or(ControlFlowTarget::Invalid(raw));
    }
    if raw < 0 {
        let variable = i64::from(raw)
            .checked_neg()
            .and_then(|value| value.checked_sub(1));
        return variable
            .and_then(|value| u16::try_from(value).ok())
            .map(ControlFlowTarget::DynamicVariable)
            .unwrap_or(ControlFlowTarget::Invalid(raw));
    }
    ControlFlowTarget::None
}

fn positive_index(raw: i32, entry_count: usize) -> Option<usize> {
    if raw <= 0 {
        return None;
    }
    let index = usize::try_from(raw).ok()?.checked_sub(1)?;
    if index < entry_count {
        Some(index)
    } else {
        None
    }
}

fn direct_index(raw: i32, entry_count: usize) -> Option<usize> {
    let index = usize::try_from(raw).ok()?;
    if index < entry_count {
        Some(index)
    } else {
        None
    }
}

fn add_root(roots: &mut Vec<ScriptRoot>, entry: usize, kind: ScriptRootKind) {
    if roots
        .iter()
        .any(|root| root.entry == entry && root.kind == kind)
    {
        return;
    }
    let name = root_name(entry, &kind);
    roots.push(ScriptRoot { entry, kind, name });
}

fn root_name(entry: usize, kind: &ScriptRootKind) -> String {
    match kind {
        ScriptRootKind::Section { index } => format!("section_{index}"),
        ScriptRootKind::Callback { callback } => callback.name().to_string(),
        ScriptRootKind::Page { index, handler } => {
            let suffix = match handler {
                PageHandler::Pre => "pre",
                PageHandler::Show => "show",
                PageHandler::Leave => "leave",
            };
            format!("page_{index}_{suffix}")
        }
        ScriptRootKind::CalledFunction => format!("func_{entry}"),
        ScriptRootKind::CalledLabel => format!("label_func_{entry}"),
        ScriptRootKind::Label => format!("label_{entry}"),
        ScriptRootKind::EntryZero => "entry_0".to_string(),
    }
}

fn build_blocks(
    entry_count: usize,
    leaders: &BTreeSet<usize>,
) -> (Vec<BasicBlock>, Vec<Option<usize>>) {
    let ordered: Vec<_> = leaders
        .iter()
        .copied()
        .filter(|leader| *leader < entry_count)
        .collect();
    let mut blocks = Vec::new();
    let mut entry_to_block = vec![None; entry_count];

    for (id, start) in ordered.iter().copied().enumerate() {
        let end = ordered
            .get(id.saturating_add(1))
            .copied()
            .unwrap_or(entry_count);
        for entry in start..end {
            if let Some(slot) = entry_to_block.get_mut(entry) {
                *slot = Some(id);
            }
        }
        blocks.push(BasicBlock {
            id,
            function: None,
            start,
            end,
        });
    }

    (blocks, entry_to_block)
}

fn materialize_edges(
    pending: Vec<PendingEdge>,
    entry_to_block: &[Option<usize>],
) -> Vec<ControlFlowEdge> {
    let mut edges = Vec::new();
    for edge in pending {
        let Some(Some(source_block)) = entry_to_block.get(edge.source_entry) else {
            continue;
        };
        let target_block = match edge.target {
            ControlFlowTarget::Entry(entry) => entry_to_block.get(entry).copied().flatten(),
            _ => None,
        };
        edges.push(ControlFlowEdge {
            source_block: *source_block,
            source_entry: edge.source_entry,
            kind: edge.kind,
            target_block,
            target: edge.target,
        });
    }
    edges
}

fn build_functions(
    roots: &[ScriptRoot],
    blocks: &[BasicBlock],
    edges: &[ControlFlowEdge],
) -> Vec<ScriptFunction> {
    let mut by_entry: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (root_id, root) in roots.iter().enumerate() {
        by_entry.entry(root.entry).or_default().push(root_id);
    }

    let root_entries: BTreeSet<_> = by_entry.keys().copied().collect();
    let mut functions = Vec::new();
    for (entry, root_ids) in by_entry {
        let Some(start_block) = block_for_entry(blocks, entry) else {
            continue;
        };
        let blocks_for_function = reachable_blocks(start_block, &root_entries, blocks, edges);
        let name = root_ids
            .first()
            .and_then(|root_id| roots.get(*root_id))
            .map(|root| root.name.clone())
            .unwrap_or_else(|| format!("func_{entry}"));
        let id = functions.len();
        let end = blocks_for_function
            .iter()
            .filter_map(|block_id| blocks.get(*block_id).map(|block| block.end))
            .max();
        functions.push(ScriptFunction {
            id,
            name,
            entry,
            end,
            roots: root_ids,
            blocks: blocks_for_function,
        });
    }
    functions
}

fn reachable_blocks(
    start_block: usize,
    root_entries: &BTreeSet<usize>,
    blocks: &[BasicBlock],
    edges: &[ControlFlowEdge],
) -> Vec<usize> {
    let mut seen = BTreeSet::new();
    let mut queue = VecDeque::new();
    seen.insert(start_block);
    queue.push_back(start_block);

    while let Some(block) = queue.pop_front() {
        for edge in edges.iter().filter(|edge| edge.source_block == block) {
            if matches!(edge.kind, EdgeKind::Call { .. }) {
                continue;
            }
            let Some(target_block) = edge.target_block else {
                continue;
            };
            let Some(target) = blocks.get(target_block) else {
                continue;
            };
            if target_block != start_block && root_entries.contains(&target.start) {
                continue;
            }
            if seen.insert(target_block) {
                queue.push_back(target_block);
            }
        }
    }

    seen.into_iter().collect()
}

fn block_for_entry(blocks: &[BasicBlock], entry: usize) -> Option<usize> {
    blocks
        .iter()
        .find(|block| entry >= block.start && entry < block.end)
        .map(|block| block.id)
}

fn assign_block_functions(blocks: &mut [BasicBlock], functions: &mut [ScriptFunction]) {
    let mut block_to_function = BTreeMap::new();
    for function in functions.iter() {
        for block in &function.blocks {
            block_to_function.entry(*block).or_insert(function.id);
        }
    }
    for block in blocks {
        block.function = block_to_function.get(&block.id).copied();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(which: i32, offsets: [i32; 6]) -> Entry<'static> {
        let mut data = Vec::new();
        data.extend_from_slice(&which.to_le_bytes());
        for offset in offsets {
            data.extend_from_slice(&offset.to_le_bytes());
        }
        let leaked: &'static [u8] = Vec::leak(data);
        Entry::parse(leaked).unwrap()
    }

    fn edge_kinds(which: i32, offsets: [i32; 6]) -> Vec<EdgeKind> {
        classify_opcode(which, &make_entry(which, offsets), 3, 10)
            .edges
            .into_iter()
            .map(|edge| edge.kind)
            .collect()
    }

    #[test]
    fn control_flow_opcodes_are_classified() {
        assert_eq!(edge_kinds(opcode::EW_RET, [0; 6]), vec![EdgeKind::Return]);
        assert_eq!(edge_kinds(opcode::EW_ABORT, [0; 6]), vec![EdgeKind::Abort]);
        assert_eq!(edge_kinds(opcode::EW_QUIT, [0; 6]), vec![EdgeKind::Quit]);
        assert_eq!(
            edge_kinds(opcode::EW_NOP, [5, 0, 0, 0, 0, 0]),
            vec![EdgeKind::Jump]
        );
        assert_eq!(
            edge_kinds(opcode::EW_CALL, [5, 0, 0, 0, 0, 0]),
            vec![EdgeKind::Call { label_style: false }, EdgeKind::Fallthrough]
        );
        assert_eq!(
            edge_kinds(opcode::EW_IFFILEEXISTS, [0, 5, 6, 0, 0, 0]),
            vec![
                EdgeKind::Branch {
                    condition: BranchCondition::IfFileExists,
                },
                EdgeKind::Branch {
                    condition: BranchCondition::IfFileMissing,
                },
            ]
        );
        assert_eq!(
            edge_kinds(opcode::EW_IFFLAG, [5, 6, 0, 0, 0, 0]),
            vec![
                EdgeKind::Branch {
                    condition: BranchCondition::IfFlagSet,
                },
                EdgeKind::Branch {
                    condition: BranchCondition::IfFlagClear,
                },
            ]
        );
        assert_eq!(
            edge_kinds(opcode::EW_MESSAGEBOX, [0, 0, 1, 5, 2, 6]),
            vec![
                EdgeKind::Branch {
                    condition: BranchCondition::MessageBoxButton1 { button: 1 },
                },
                EdgeKind::Branch {
                    condition: BranchCondition::MessageBoxButton2 { button: 2 },
                },
                EdgeKind::Fallthrough,
            ]
        );
        assert_eq!(
            edge_kinds(opcode::EW_STRCMP, [0, 0, 5, 6, 0, 0]),
            vec![
                EdgeKind::Branch {
                    condition: BranchCondition::StrEqual,
                },
                EdgeKind::Branch {
                    condition: BranchCondition::StrNotEqual,
                },
            ]
        );
        assert_eq!(
            edge_kinds(opcode::EW_INTCMP, [0, 0, 5, 6, 7, 0]),
            vec![
                EdgeKind::Branch {
                    condition: BranchCondition::IntEqual,
                },
                EdgeKind::Branch {
                    condition: BranchCondition::IntLess,
                },
                EdgeKind::Branch {
                    condition: BranchCondition::IntGreater,
                },
            ]
        );
        assert_eq!(
            edge_kinds(opcode::EW_ISWINDOW, [0, 5, 6, 0, 0, 0]),
            vec![
                EdgeKind::Branch {
                    condition: BranchCondition::IsWindow,
                },
                EdgeKind::Branch {
                    condition: BranchCondition::IsNotWindow,
                },
            ]
        );
    }

    #[test]
    fn target_resolution_distinguishes_static_dynamic_and_invalid() {
        assert_eq!(resolve_target(5, 10), ControlFlowTarget::Entry(4));
        assert_eq!(
            resolve_target(-22, 10),
            ControlFlowTarget::DynamicVariable(21)
        );
        assert_eq!(resolve_target(0, 10), ControlFlowTarget::None);
        assert_eq!(resolve_target(20, 10), ControlFlowTarget::Invalid(20));
    }
}
