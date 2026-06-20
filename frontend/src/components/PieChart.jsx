import { useEffect, useRef, useState } from 'react';
import { formatSize, getColor, shortPath } from '../utils.js';

/**
 * Interactive donut pie chart using Canvas API.
 * Props:
 *   data: Array<{ name, size, type, path }>
 *   totalSize: number
 *   onSliceClick: (item) => void
 *   highlightedIndex: number | null
 *   onHighlight: (index | null) => void
 */
export default function PieChart({ data, totalSize, onSliceClick, highlightedIndex, onHighlight }) {
  const canvasRef = useRef(null);
  const animRef = useRef(null);
  const progressRef = useRef(0);
  const [tooltip, setTooltip] = useState(null);
  const slicesRef = useRef([]);

  // Animation on mount / data change
  useEffect(() => {
    progressRef.current = 0;
    const startTime = performance.now();
    const duration = 700;

    const animate = (now) => {
      const elapsed = now - startTime;
      progressRef.current = Math.min(elapsed / duration, 1);
      // Ease out cubic
      const t = 1 - Math.pow(1 - progressRef.current, 3);
      drawChart(t);
      if (progressRef.current < 1) {
        animRef.current = requestAnimationFrame(animate);
      }
    };

    animRef.current = requestAnimationFrame(animate);
    return () => cancelAnimationFrame(animRef.current);
  }, [data, highlightedIndex]);

  function drawChart(progress = 1) {
    const canvas = canvasRef.current;
    if (!canvas || !data || data.length === 0) return;

    const dpr = window.devicePixelRatio || 1;
    const size = canvas.parentElement.clientWidth;
    canvas.width = size * dpr;
    canvas.height = size * dpr;
    canvas.style.width = size + 'px';
    canvas.style.height = size + 'px';

    const ctx = canvas.getContext('2d');
    ctx.scale(dpr, dpr);
    ctx.clearRect(0, 0, size, size);

    const cx = size / 2;
    const cy = size / 2;
    const outerR = (size / 2) * 0.88;
    const innerR = outerR * 0.55;
    const gap = 0.012; // gap in radians between slices

    let startAngle = -Math.PI / 2;
    const newSlices = [];

    // Filter zero-size entries
    const validData = data.filter(d => d.size > 0);
    const displayedTotal = validData.reduce((s, d) => s + d.size, 0);

    validData.forEach((item, i) => {
      const fraction = item.size / displayedTotal;
      const fullSweep = fraction * 2 * Math.PI * progress;
      const sweepAngle = Math.max(fullSweep - gap, 0);
      const endAngle = startAngle + fullSweep;
      const midAngle = startAngle + fullSweep / 2;

      const isHighlighted = highlightedIndex === i;
      const offset = isHighlighted ? 10 : 0;
      const ox = Math.cos(midAngle) * offset;
      const oy = Math.sin(midAngle) * offset;

      const color = getColor(i);

      // Shadow for highlighted
      if (isHighlighted) {
        ctx.save();
        ctx.shadowColor = color;
        ctx.shadowBlur = 20;
      }

      ctx.beginPath();
      ctx.moveTo(cx + ox, cy + oy);
      ctx.arc(cx + ox, cy + oy, outerR, startAngle, startAngle + sweepAngle);
      ctx.arc(cx + ox, cy + oy, innerR, startAngle + sweepAngle, startAngle, true);
      ctx.closePath();

      ctx.fillStyle = color;
      ctx.globalAlpha = isHighlighted ? 1 : highlightedIndex !== null ? 0.55 : 1;
      ctx.fill();

      if (isHighlighted) {
        ctx.restore();
        // Outer ring
        ctx.beginPath();
        ctx.arc(cx + ox, cy + oy, outerR + 4, startAngle, startAngle + sweepAngle);
        ctx.arc(cx + ox, cy + oy, outerR, startAngle + sweepAngle, startAngle, true);
        ctx.closePath();
        ctx.fillStyle = color + '40';
        ctx.globalAlpha = 1;
        ctx.fill();
      }

      ctx.globalAlpha = 1;

      newSlices.push({ startAngle, sweepAngle: fullSweep, midAngle, cx: cx + ox, cy: cy + oy, outerR, innerR, item, i });
      startAngle = endAngle;
    });

    slicesRef.current = newSlices;
  }

  function getHitSlice(x, y) {
    for (const s of slicesRef.current) {
      const dx = x - s.cx;
      const dy = y - s.cy;
      const dist = Math.sqrt(dx * dx + dy * dy);
      if (dist < s.innerR || dist > s.outerR + 12) continue;
      let angle = Math.atan2(dy, dx);
      // Normalize angle to [startAngle, startAngle + sweep]
      let start = s.startAngle;
      let end = start + s.sweepAngle;
      // Normalize angle into same range
      while (angle < start) angle += 2 * Math.PI;
      while (angle > end + 0.01) angle -= 2 * Math.PI;
      if (angle >= start && angle <= end) return s;
    }
    return null;
  }

  function handleMouseMove(e) {
    const canvas = canvasRef.current;
    const rect = canvas.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;
    const hit = getHitSlice(x, y);
    if (hit) {
      onHighlight(hit.i);
      setTooltip({
        x: e.clientX + 16,
        y: e.clientY + 8,
        item: hit.item,
        pct: ((hit.item.size / (totalSize || 1)) * 100).toFixed(1),
      });
      canvas.style.cursor = hit.item.type === 'directory' ? 'pointer' : 'default';
    } else {
      onHighlight(null);
      setTooltip(null);
      canvas.style.cursor = 'default';
    }
  }

  function handleClick(e) {
    const canvas = canvasRef.current;
    const rect = canvas.getBoundingClientRect();
    const x = e.clientX - rect.left;
    const y = e.clientY - rect.top;
    const hit = getHitSlice(x, y);
    if (hit && hit.item.type === 'directory') {
      onSliceClick(hit.item);
    }
  }

  function handleMouseLeave() {
    onHighlight(null);
    setTooltip(null);
  }

  const currentItem = highlightedIndex !== null ? data[highlightedIndex] : null;

  return (
    <div style={{ position: 'relative' }}>
      <div className="chart-wrapper">
        <canvas
          ref={canvasRef}
          onMouseMove={handleMouseMove}
          onClick={handleClick}
          onMouseLeave={handleMouseLeave}
          style={{ display: 'block', borderRadius: '50%' }}
        />
        <div className="chart-center-info">
          {currentItem ? (
            <>
              <span className="size-label">{formatSize(currentItem.size)}</span>
              <span className="size-name" title={currentItem.name}>{shortPath(currentItem.name)}</span>
              <span style={{ fontSize: 10, color: 'var(--text-muted)', marginTop: 2 }}>
                {((currentItem.size / (totalSize || 1)) * 100).toFixed(1)}%
              </span>
            </>
          ) : (
            <>
              <span className="size-label">{formatSize(totalSize)}</span>
              <span className="size-name">Total</span>
            </>
          )}
        </div>
      </div>

      {tooltip && (
        <div
          className="tooltip-overlay"
          style={{ left: tooltip.x, top: tooltip.y, position: 'fixed' }}
        >
          <div className="tooltip-name">{tooltip.item.name}</div>
          <div className="tooltip-size">{formatSize(tooltip.item.size)}</div>
          <div className="tooltip-pct">{tooltip.pct}% of total</div>
          {tooltip.item.type === 'directory' && (
            <div style={{ fontSize: 10, color: 'var(--text-muted)', marginTop: 4 }}>
              Click to explore →
            </div>
          )}
        </div>
      )}
    </div>
  );
}
