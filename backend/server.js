import express from 'express';
import fs from 'fs';
import path from 'path';
import { fileURLToPath } from 'url';
import cors from 'cors';
import os from 'os';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const app = express();
const PORT = 3001;

app.use(cors());
app.use(express.json());

/**
 * Recursively computes total size of a directory entry.
 * Returns an object with name, type, size, path, and children (for dirs).
 */
function getEntryInfo(entryPath, depth = 0, maxDepth = 1) {
  let stats;
  try {
    stats = fs.statSync(entryPath);
  } catch {
    return null;
  }

  const name = path.basename(entryPath);

  if (stats.isFile()) {
    return {
      name,
      type: 'file',
      size: stats.size,
      path: entryPath,
      ext: path.extname(name).toLowerCase() || 'none',
    };
  }

  if (stats.isDirectory()) {
    let children = [];
    let totalSize = 0;

    try {
      const entries = fs.readdirSync(entryPath);
      for (const entry of entries) {
        const childPath = path.join(entryPath, entry);
        const childInfo = getEntryInfo(childPath, depth + 1, maxDepth);
        if (childInfo) {
          totalSize += childInfo.size;
          if (depth < maxDepth) {
            children.push(childInfo);
          }
        }
      }
    } catch {
      // Permission denied or other errors
    }

    return {
      name,
      type: 'directory',
      size: totalSize,
      path: entryPath,
      children: children.sort((a, b) => b.size - a.size),
    };
  }

  return null;
}

/**
 * GET /api/scan?dir=<path>
 * Scans the given directory (one level deep) and returns children with sizes.
 */
app.get('/api/scan', (req, res) => {
  let targetDir = req.query.dir;

  if (!targetDir) {
    // Default to user's home directory
    targetDir = os.homedir();
  }

  // Normalize path
  targetDir = path.normalize(targetDir);

  // Security: check the path exists and is a directory
  let stats;
  try {
    stats = fs.statSync(targetDir);
  } catch {
    return res.status(400).json({ error: `Path not found: ${targetDir}` });
  }

  if (!stats.isDirectory()) {
    return res.status(400).json({ error: 'Path is not a directory' });
  }

  // Get top-level children
  let entries = [];
  try {
    const names = fs.readdirSync(targetDir);
    for (const name of names) {
      const fullPath = path.join(targetDir, name);
      const info = getEntryInfo(fullPath, 0, 0); // depth=0 means only compute total size
      if (info) {
        entries.push(info);
      }
    }
  } catch (e) {
    return res.status(500).json({ error: `Cannot read directory: ${e.message}` });
  }

  // Sort by size descending
  entries.sort((a, b) => b.size - a.size);

  const totalSize = entries.reduce((sum, e) => sum + e.size, 0);

  return res.json({
    path: targetDir,
    totalSize,
    entries,
    parent: path.dirname(targetDir) !== targetDir ? path.dirname(targetDir) : null,
  });
});

/**
 * GET /api/drives
 * Lists available drives/roots.
 */
app.get('/api/drives', (req, res) => {
  if (process.platform === 'win32') {
    // On Windows, list common drive letters
    const drives = [];
    for (let i = 67; i <= 90; i++) {
      const drive = `${String.fromCharCode(i)}:\\`;
      try {
        fs.statSync(drive);
        drives.push(drive);
      } catch {
        // Drive doesn't exist
      }
    }
    return res.json({ drives });
  } else {
    return res.json({ drives: ['/'] });
  }
});

app.listen(PORT, () => {
  console.log(`🚀 Disk Explorer backend running at http://localhost:${PORT}`);
  console.log(`   Press Ctrl+C to stop.`);
});
