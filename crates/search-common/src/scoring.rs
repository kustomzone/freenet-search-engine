use crate::types::Status;

/// Integer TF-IDF score (x10000 scaling, no floats).
/// tf = (term_count * 10000) / total_terms
/// idf = integer_log2(total_docs / docs_with_term) scaled by 10000, with minimum of 10000 (log2(1)=0 baseline)
/// score = (tf * idf) / 10000
pub fn integer_tf_idf(
    term_count: u32,
    total_terms: u32,
    total_docs: u32,
    docs_with_term: u32,
) -> u32 {
    if total_terms == 0 || docs_with_term == 0 {
        return 0;
    }
    let tf = (term_count as u64 * 10000) / total_terms as u64;
    // idf = log2(total_docs / docs_with_term) * 10000
    // When total_docs == docs_with_term, log2(1) = 0, so we add a baseline of 10000
    // to ensure the score is always > 0 when term_count > 0
    let ratio = total_docs as u64 * 10000 / docs_with_term as u64;
    let idf = integer_log2_scaled(ratio) + 10000;
    ((tf * idf) / 10000) as u32
}

/// Approximate log2(x/10000) * 10000, where x is already multiplied by 10000.
fn integer_log2_scaled(x: u64) -> u64 {
    if x <= 10000 {
        return 0;
    }
    // x = ratio * 10000, so log2(x) = log2(ratio) + log2(10000)
    // log2(10000) ~ 13.29
    // We want log2(ratio) * 10000
    let bits = 64 - x.leading_zeros() as u64;
    let log2_x = bits.saturating_sub(1); // approximate log2(x)
                                         // log2(ratio) = log2(x) - log2(10000) ≈ log2(x) - 13.3
                                         // Scale by 10000
    log2_x.saturating_sub(13) * 10000
}

/// Passive rank score using weighted attestations, version, subscribers.
/// All computed with integer arithmetic (x10000 scaling).
pub fn rank_score(
    weighted_attestations: u32,
    version: u64,
    subscribers: u32,
    status: &Status,
) -> u32 {
    let att_score = log_scale(weighted_attestations as u64);
    let ver_score = log_scale(version);
    let sub_score = log_scale(subscribers as u64);

    let base = (att_score * 4000 + ver_score * 3000 + sub_score * 3000) / 10000;

    let bonus: i64 = match status {
        Status::Confirmed => 3000,
        Status::Pending => 0,
        Status::Disputed => -2000,
        Status::Expired => -1000,
    };

    let result = base as i64 + bonus;
    result.max(0) as u32
}

fn log_scale(x: u64) -> u64 {
    if x == 0 {
        return 0;
    }
    let bits = 64 - x.leading_zeros() as u64;
    bits * 500
}

/// Combined final score: relevance * 7000 / 10000 + rank * 3000 / 10000.
pub fn combined_score(relevance: u32, rank: u32) -> u32 {
    let r = relevance as u64;
    let k = rank as u64;
    ((r * 7000 + k * 3000) / 10000) as u32
}
