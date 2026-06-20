// Utility: format bytes to human readable
export function formatSize(bytes) {
  if (bytes === 0) return '0 B';
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

// Utility: shorten a path for display
export function shortPath(p) {
  if (!p) return '';
  const parts = p.replace(/\\/g, '/').split('/');
  return parts[parts.length - 1] || p;
}

// Utility: build breadcrumb segments from a full path
export function buildBreadcrumbs(currentPath) {
  if (!currentPath) return [];
  const sep = currentPath.includes('\\') ? '\\' : '/';
  const parts = currentPath.split(sep).filter(Boolean);
  const crumbs = [];

  // Handle Windows drive letter e.g. "C:"
  if (currentPath.match(/^[A-Za-z]:\\/)) {
    const drive = parts[0] + sep;
    crumbs.push({ label: drive, path: drive });
    for (let i = 1; i < parts.length; i++) {
      crumbs.push({
        label: parts[i],
        path: crumbs[i - 1].path + parts[i] + (i < parts.length - 1 ? sep : ''),
      });
    }
  } else {
    // Unix-like
    let acc = '';
    for (let i = 0; i < parts.length; i++) {
      acc += sep + parts[i];
      crumbs.push({ label: parts[i], path: acc });
    }
  }

  return crumbs;
}

// Color palette for the chart (vibrant but harmonious)
export const CHART_COLORS = [
  '#6366f1', // indigo
  '#8b5cf6', // violet
  '#ec4899', // pink
  '#f43f5e', // rose
  '#f97316', // orange
  '#eab308', // yellow
  '#10b981', // emerald
  '#06b6d4', // cyan
  '#3b82f6', // blue
  '#84cc16', // lime
  '#a855f7', // purple
  '#14b8a6', // teal
  '#f59e0b', // amber
  '#ef4444', // red
  '#22c55e', // green
  '#0ea5e9', // sky
];

export function getColor(index) {
  return CHART_COLORS[index % CHART_COLORS.length];
}

export function getFileIcon(entry) {
  if (entry.type === 'directory') return '📁';
  const ext = (entry.ext || '').toLowerCase();
  if (['.mp4', '.mkv', '.avi', '.mov', '.wmv'].includes(ext)) return '🎬';
  if (['.mp3', '.flac', '.wav', '.aac', '.ogg'].includes(ext)) return '🎵';
  if (['.jpg', '.jpeg', '.png', '.gif', '.webp', '.bmp', '.svg'].includes(ext)) return '🖼️';
  if (['.zip', '.rar', '.7z', '.tar', '.gz'].includes(ext)) return '📦';
  if (['.pdf'].includes(ext)) return '📄';
  if (['.doc', '.docx'].includes(ext)) return '📝';
  if (['.xls', '.xlsx', '.csv'].includes(ext)) return '📊';
  if (['.ppt', '.pptx'].includes(ext)) return '📋';
  if (['.exe', '.msi', '.dmg', '.app'].includes(ext)) return '⚙️';
  if (['.js', '.ts', '.jsx', '.tsx', '.py', '.rb', '.go', '.rs', '.cpp', '.c', '.h', '.java', '.cs', '.php'].includes(ext)) return '💻';
  if (['.json', '.xml', '.yaml', '.yml', '.toml', '.env'].includes(ext)) return '📐';
  if (['.html', '.css', '.scss'].includes(ext)) return '🌐';
  if (['.txt', '.md', '.log'].includes(ext)) return '📃';
  if (['.iso', '.img'].includes(ext)) return '💿';
  return '📄';
}
