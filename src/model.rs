/// What a trace read produces.
///
/// One value holding two sorted lists of path strings. The first
/// list is the read set, holding resolved paths relative to the
/// root. The second list is the outside reads, holding resolved
/// absolute paths that landed outside the root. This is the
/// counterpart of the pair the Python parse entry returns, and it
/// is the whole of what a trace read produces.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TraceRead {
    pub reads: Vec<String>,
    pub outside_reads: Vec<String>,
}

/// The verdict word for a check whose reads touched its surface.
pub const VERDICT_PROVEN: &str = "proven";

/// The verdict word for a check whose reads never touched its surface.
pub const VERDICT_UNPROVEN: &str = "unproven";

/// The verdict word for a check whose read set is empty.
pub const VERDICT_EMPTY: &str = "empty read set";

/// One check as a config declares it.
///
/// The counterpart of the Python config module's Check. The
/// name and job are the texts the config carried, the surface
/// is the claimed surface entries in config order, and the
/// trace fields stand only when the config carried a trace
/// field, the text as spelled in the config and the path it
/// resolved to against the config file's folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Check {
    pub name: String,
    pub job: String,
    pub surface: Vec<String>,
    pub trace: Option<String>,
    pub trace_path: Option<std::path::PathBuf>,
}

/// Where a check's read set came from.
///
/// The kind is one of the source kind words the analysis
/// module names, and the detail is the free text that stands
/// beside it, a trace file name or a run command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceSource {
    pub kind: String,
    pub detail: String,
}

/// The judged outcome for one check.
///
/// The counterpart of the Python analysis module's
/// CheckResult. The surface is the check's claimed surface,
/// the read set is what the run read in the order it was
/// handed over, the matched list is the read set's paths that
/// fall in the surface in read set order, and the verdict is
/// one of the three verdict words. The trace source stands
/// only when a judgement supplied one, and the outside reads
/// are carried untouched from the judgement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckResult {
    pub name: String,
    pub job: String,
    pub surface: Vec<String>,
    pub read_set: Vec<String>,
    pub matched: Vec<String>,
    pub verdict: String,
    pub trace_source: Option<TraceSource>,
    pub outside_reads: Vec<String>,
}
