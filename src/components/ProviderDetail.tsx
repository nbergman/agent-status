import type { AgentStats, Bucket } from "../types";

function InfoIcon() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round">
      <circle cx={12} cy={12} r={10} />
      <path d="M12 16v-4M12 8h.01" />
    </svg>
  );
}

export function ProviderDetail({
  title,
  meta,
  stats,
}: {
  title: string;
  meta: string;
  stats: AgentStats;
}) {
  return (
    <>
      <div className="sec-head">
        <h2>{title}</h2>
        <span className="meta">{meta}</span>
      </div>
      <div className="budget">
        {stats.buckets.map((b, i) => (
          <LimitRow bucket={b} key={b.name} first={i === 0} />
        ))}
        <div className="budget-foot" style={{ marginTop: stats.buckets.length === 0 ? 0 : undefined }}>
          <span className="used">{stats.sessions} sessions</span>
          <span className="rem">{stats.activeDays} active days</span>
        </div>
        {stats.planLabel !== "—" && (
          <div className="budget-foot">
            <span className="used">plan / model</span>
            <span className="rem">{stats.planLabel}</span>
          </div>
        )}
        {stats.totalTokens !== "—" && (
          <div className="budget-foot">
            <span className="used">context tokens</span>
            <span className="rem">{stats.totalTokens}</span>
          </div>
        )}
        <div className="budget-foot">
          <span className="used">last seen</span>
          <span className="rem">{stats.last}</span>
        </div>
      </div>
      <div className="note">
        <InfoIcon />
        <p>{stats.note}</p>
      </div>
    </>
  );
}

function LimitRow({ bucket, first }: { bucket: Bucket; first: boolean }) {
  return (
    <div className="budget-foot" style={{ marginTop: first ? 0 : undefined }}>
      <span className="used">
        {bucket.name}
        {bucket.sub ? ` · ${bucket.sub}` : ""}
      </span>
      <span className="rem">
        {bucket.usedPct}% used · resets {bucket.reset}
      </span>
    </div>
  );
}
