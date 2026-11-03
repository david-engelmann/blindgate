/// What a trace read produces.
///
/// One value holding two sorted lists of path strings. The first
/// list is the read set, holding resolved paths relative to the
/// root. The second list is the outside reads, holding resolved
/// absolute paths that landed outside the root. This is the
/// counterpart of the pair the Python parse entry returns, and it
/// is the whole of what a trace read produces in port slice 1.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TraceRead {
    pub reads: Vec<String>,
    pub outside_reads: Vec<String>,
}
