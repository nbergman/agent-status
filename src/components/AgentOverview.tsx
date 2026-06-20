import { Meter } from "./Meter";
import type { AgentStats } from "../types";

function InfoIcon() {
  return (
    <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round">
      <circle cx={12} cy={12} r={10} />
      <path d="M12 16v-4M12 8h.01" />
    </svg>
  );
}

export function AgentOverview({
  title,
  meta,
  stats,
  minimal,
  liveLabel,
}: {
  title: string;
  meta: string;
  stats: AgentStats;
  minimal: boolean;
  liveLabel: string;
}) {
  const hasBuckets = stats.buckets.length > 0;

  return (
    <>
      <div className="sec-head">
        <h2>{title}</h2>
        <span className="meta">{meta}</span>
      </div>

      {hasBuckets ? (
        <>
          <div className="kpis">
            {stats.buckets.slice(0, 3).map((b, i) => (
              <div className={`kpi ${["accent", "ok", ""][i]}`} key={b.name}>
                <div className="k-label">{b.name}</div>
                <div className="k-num">{b.usedPct}%</div>
                <div className="k-sub">resets {b.reset}</div>
              </div>
            ))}
          </div>

          {!minimal && (
            <>
              <div className="sec-head">
                <h2>Usage</h2>
                <span className="meta">{liveLabel}</span>
              </div>
              <div className="meters">
                {stats.buckets.map((b) => (
                  <Meter bucket={b} key={b.name} />
                ))}
              </div>
            </>
          )}
        </>
      ) : (
        <div className="connect-card">
          <p className="connect-title">No {title} usage data yet</p>
          <p className="connect-sub">{stats.note}</p>
        </div>
      )}

      {!minimal && (
        <>
          {stats.models.length > 0 && (
            <>
              <div className="sec-head">
                <h2>By model</h2>
                <span className="meta">{stats.planLabel}</span>
              </div>
              <div className="models">
                {stats.models.map((m) => (
                  <div className="model-row" key={m.key}>
                    <span className="name">{m.name}</span>
                    <div className="mtrack">
                      <div className={`mfill ${m.key}`} style={{ width: `${m.pct}%` }} />
                    </div>
                    <span className="mval">
                      <b>{m.tokens}</b>
                    </span>
                  </div>
                ))}
              </div>
            </>
          )}

          <div className="sec-head">
            <h2>Local activity</h2>
            <span className="meta">session logs</span>
          </div>
          <div className="budget">
            <div className="budget-foot" style={{ marginTop: 0 }}>
              <span className="used">{stats.sessions} sessions</span>
              <span className="rem">{stats.activeDays} active projects</span>
            </div>
            <div className="budget-foot">
              <span className="used">last seen</span>
              <span className="rem">{stats.last}</span>
            </div>
            {stats.totalTokens !== "—" && (
              <div className="budget-foot">
                <span className="used">context tokens</span>
                <span className="rem">{stats.totalTokens}</span>
              </div>
            )}
          </div>

          <div className="note">
            <InfoIcon />
            <p>{stats.note}</p>
          </div>
        </>
      )}
    </>
  );
}
