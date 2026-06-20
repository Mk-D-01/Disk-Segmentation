import { useState, useEffect, useCallback } from 'react';
import PieChart from './components/PieChart.jsx';
import { formatSize, buildBreadcrumbs, getColor, getFileIcon, shortPath } from './utils.js';

const API = 'http://localhost:3001';

export default function App() {
  const [currentPath, setCurrentPath] = useState('');
  const [inputPath, setInputPath] = useState('');
  const [data, setData] = useState(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState(null);
  const [highlightedIndex, setHighlightedIndex] = useState(null);
  const [drives, setDrives] = useState([]);
  const [history, setHistory] = useState([]);

  // Fetch available drives on mount
  useEffect(() => {
    fetch(`${API}/api/drives`)
      .then(r => r.json())
      .then(d => setDrives(d.drives || []))
      .catch(() => {});
  }, []);

  const scanDir = useCallback(async (dirPath, pushHistory = true) => {
    if (!dirPath) return;
    setLoading(true);
    setError(null);
    setHighlightedIndex(null);
    try {
      const res = await fetch(`${API}/api/scan?dir=${encodeURIComponent(dirPath)}`);
      if (!res.ok) {
        const err = await res.json();
        throw new Error(err.error || 'Failed to scan directory');
      }
      const json = await res.json();
      if (pushHistory && currentPath) {
        setHistory(h => [...h, currentPath]);
      }
      setCurrentPath(json.path);
      setInputPath(json.path);
      setData(json);
    } catch (e) {
      setError(e.message);
    } finally {
      setLoading(false);
    }
  }, [currentPath]);

  const handleGoBack = () => {
    if (history.length === 0) return;
    const prev = history[history.length - 1];
    setHistory(h => h.slice(0, -1));
    scanDir(prev, false);
  };

  const handleSubmit = (e) => {
    e.preventDefault();
    scanDir(inputPath);
  };

  const handleSliceClick = (item) => {
    if (item.type === 'directory') {
      scanDir(item.path);
    }
  };

  const handleFileItemClick = (item) => {
    if (item.type === 'directory') {
      scanDir(item.path);
    }
  };

  const breadcrumbs = buildBreadcrumbs(currentPath);

  // Prepare display data — group tiny items as "Others"
  let displayEntries = [];
  if (data) {
    const sorted = [...(data.entries || [])].sort((a, b) => b.size - a.size);
    const TOP = 15;
    if (sorted.length > TOP) {
      const top = sorted.slice(0, TOP);
      const others = sorted.slice(TOP);
      const othersSize = others.reduce((s, e) => s + e.size, 0);
      displayEntries = [...top];
      if (othersSize > 0) {
        displayEntries.push({
          name: `+${others.length} more items`,
          type: 'other',
          size: othersSize,
          path: null,
        });
      }
    } else {
      displayEntries = sorted;
    }
  }

  return (
    <div className="app">
      {/* ── Header ── */}
      <header className="header">
        <div className="header-logo">
          <div className="icon">🗂️</div>
          <h1>DiskLens</h1>
        </div>

        <form className="path-bar" onSubmit={handleSubmit} style={{ flex: 1 }}>
          <span style={{ color: 'var(--text-muted)', fontSize: 14 }}>📂</span>
          <input
            type="text"
            value={inputPath}
            onChange={e => setInputPath(e.target.value)}
            placeholder="Enter a directory path (e.g. C:\\Users)"
            spellCheck={false}
          />
          <button type="submit" className="btn btn-primary" disabled={loading}>
            {loading ? '⟳' : 'Scan'}
          </button>
        </form>

        <button
          className="btn btn-ghost"
          onClick={handleGoBack}
          disabled={history.length === 0 || loading}
          title="Go back"
        >
          ← Back
        </button>
      </header>

      {/* ── Drive Picker (shown when no data) ── */}
      {!data && !loading && !error && (
        <div className="state-container fade-in">
          <div className="state-icon">💾</div>
          <div className="state-title">Welcome to DiskLens</div>
          <div className="state-desc">
            Enter a directory path above and click <strong>Scan</strong>, or pick a drive below to get started.
          </div>
          {drives.length > 0 && (
            <div className="drives-row" style={{ marginTop: 16 }}>
              {drives.map(d => (
                <button key={d} className="drive-chip" onClick={() => scanDir(d)}>
                  💾 {d}
                </button>
              ))}
            </div>
          )}
        </div>
      )}

      {loading && (
        <div className="state-container fade-in">
          <div className="spinner" />
          <div className="state-title">Scanning…</div>
          <div className="state-desc" style={{ fontFamily: 'Fira Code, monospace', fontSize: 12 }}>
            {inputPath}
          </div>
        </div>
      )}

      {error && !loading && (
        <div className="state-container fade-in">
          <div className="state-icon">⚠️</div>
          <div className="state-title">Could not scan directory</div>
          <div className="state-desc">{error}</div>
          <button className="btn btn-primary" onClick={() => setError(null)} style={{ marginTop: 8 }}>
            Try Again
          </button>
        </div>
      )}

      {/* ── Main Layout ── */}
      {data && !loading && !error && (
        <div className="main fade-in">
          {/* ── Chart Panel ── */}
          <aside className="chart-panel">
            <div className="chart-header">
              <h2>Space Distribution</h2>
              <div className="current-path" title={currentPath}>
                {currentPath}
              </div>
            </div>

            <div className="chart-container">
              <PieChart
                data={displayEntries}
                totalSize={data.totalSize}
                onSliceClick={handleSliceClick}
                highlightedIndex={highlightedIndex}
                onHighlight={setHighlightedIndex}
              />
            </div>

            {/* Legend */}
            <div className="legend">
              {displayEntries.map((item, i) => (
                <div
                  key={i}
                  className={`legend-item ${highlightedIndex === i ? 'active' : ''}`}
                  onMouseEnter={() => setHighlightedIndex(i)}
                  onMouseLeave={() => setHighlightedIndex(null)}
                  onClick={() => item.type === 'directory' && handleSliceClick(item)}
                  title={item.path || item.name}
                >
                  <span className="legend-dot" style={{ background: getColor(i) }} />
                  <span className="legend-name">{item.name}</span>
                  <span className="legend-size">{formatSize(item.size)}</span>
                  <span className="legend-pct">
                    {((item.size / (data.totalSize || 1)) * 100).toFixed(1)}%
                  </span>
                </div>
              ))}
            </div>
          </aside>

          {/* ── File List Panel ── */}
          <section className="list-panel">
            {/* Breadcrumb */}
            <div className="breadcrumb">
              {breadcrumbs.map((crumb, i) => (
                <span key={i} style={{ display: 'flex', alignItems: 'center', gap: 4 }}>
                  {i > 0 && <span className="breadcrumb-sep">›</span>}
                  <span
                    className={`breadcrumb-item ${i === breadcrumbs.length - 1 ? 'active' : ''}`}
                    onClick={() => i < breadcrumbs.length - 1 && scanDir(crumb.path)}
                  >
                    {crumb.label}
                  </span>
                </span>
              ))}
            </div>

            {/* List header */}
            <div className="list-header">
              <h2>Contents</h2>
              <div className="stats-row">
                <div className="stat-chip">
                  <strong>{(data.entries || []).filter(e => e.type === 'directory').length}</strong> folders
                </div>
                <div className="stat-chip">
                  <strong>{(data.entries || []).filter(e => e.type === 'file').length}</strong> files
                </div>
                <div className="stat-chip">
                  Total: <strong>{formatSize(data.totalSize)}</strong>
                </div>
              </div>
            </div>

            {/* Column headers */}
            <div className="list-table-header">
              <div style={{ width: 44 }} />
              <span>Name</span>
              <span>Usage</span>
              <span>Size</span>
              <span style={{ textAlign: 'right' }}>%</span>
            </div>

            {/* File rows */}
            <div className="file-list">
              {(data.entries || [])
                .slice()
                .sort((a, b) => b.size - a.size)
                .map((item, i) => {
                  // Find index in displayEntries for highlight sync
                  const dispIdx = displayEntries.findIndex(d => d.name === item.name && d.path === item.path);
                  const isHighlighted = dispIdx !== -1 && highlightedIndex === dispIdx;
                  const pct = data.totalSize > 0 ? (item.size / data.totalSize) * 100 : 0;

                  return (
                    <div
                      key={i}
                      className={`file-item slide-in ${isHighlighted ? 'highlighted' : ''}`}
                      style={{ animationDelay: `${Math.min(i * 20, 300)}ms`, animationFillMode: 'both', opacity: 0 }}
                      onClick={() => handleFileItemClick(item)}
                      onMouseEnter={() => dispIdx !== -1 && setHighlightedIndex(dispIdx)}
                      onMouseLeave={() => setHighlightedIndex(null)}
                      title={item.path}
                    >
                      <div
                        className="file-icon"
                        style={{
                          background: dispIdx !== -1 ? getColor(dispIdx) + '22' : 'var(--bg-card)',
                          border: `1px solid ${dispIdx !== -1 ? getColor(dispIdx) + '44' : 'var(--border-color)'}`,
                        }}
                      >
                        {getFileIcon(item)}
                      </div>

                      <div>
                        <div className="file-name">{item.name}</div>
                        <div className="file-name-sub">
                          {item.type === 'directory' ? 'Folder' : (item.ext && item.ext !== 'none' ? item.ext.toUpperCase() + ' File' : 'File')}
                        </div>
                      </div>

                      <div className="file-bar-cell">
                        <div className="file-bar-bg">
                          <div
                            className="file-bar-fill"
                            style={{
                              width: `${pct}%`,
                              background: dispIdx !== -1 ? getColor(dispIdx) : 'var(--accent-primary)',
                            }}
                          />
                        </div>
                      </div>

                      <div className="file-size">{formatSize(item.size)}</div>
                      <div className="file-pct">{pct.toFixed(1)}%</div>
                    </div>
                  );
                })}
            </div>
          </section>
        </div>
      )}
    </div>
  );
}
