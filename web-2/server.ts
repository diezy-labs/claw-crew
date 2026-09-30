import express, { Request, Response } from 'express';
import os from 'os';
import path from 'path';
import { fileURLToPath } from 'url';
import http from 'http';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const app = express();
const port = parseInt(process.env.PORT || '3000', 10);
const host = process.env.HOST || '0.0.0.0';
const isProduction = process.env.NODE_ENV === 'production';
const ollamaHost = process.env.OLLAMA_HOST || 'http://127.0.0.1:11434';

app.use(express.json());

// Helper: Discover active local IPv4 network interfaces
function getNetworkInterfaces(): { name: string; address: string; url: string }[] {
  const interfaces = os.networkInterfaces();
  const results: { name: string; address: string; url: string }[] = [];

  for (const [name, netInterface] of Object.entries(interfaces)) {
    if (!netInterface) continue;
    for (const iface of netInterface) {
      // Pick IPv4 and skip internal loopback for external access
      if (iface.family === 'IPv4') {
        results.push({
          name,
          address: iface.address,
          url: `http://${iface.address}:${port}`
        });
      }
    }
  }
  return results;
}

// -------------------------------------------------------------
// REST API LAYER (Multi-Platform Gateway)
// -------------------------------------------------------------

// 1. Healthcheck for Nginx, Docker, & Cloud Load Balancers
app.get('/api/health', (_req: Request, res: Response) => {
  res.json({
    status: 'ok',
    app: 'Fleet AI Orchestration Engine',
    uptime: process.uptime(),
    timestamp: new Date().toISOString(),
    nodeVersion: process.version,
    platform: process.platform,
    arch: process.arch
  });
});

// 2. Network Discovery for LAN & Cross-Device Pairing (Mobile, Laptop, WebView)
app.get('/api/system/network', (_req: Request, res: Response) => {
  const interfaces = getNetworkInterfaces();
  const lanInterfaces = interfaces.filter((i) => i.address !== '127.0.0.1');

  res.json({
    hostname: os.hostname(),
    port,
    host,
    interfaces,
    preferredLanUrl: lanInterfaces.length > 0 ? lanInterfaces[0].url : `http://localhost:${port}`,
    isProduction,
    platform: process.platform,
    memoryUsageMB: Math.round(process.memoryUsage().rss / (1024 * 1024))
  });
});

// 3. Local Model (Ollama) Proxy Bridge - Allows Mobile/LAN to reach local Ollama without CORS
app.get('/api/providers/ollama/status', async (_req: Request, res: Response) => {
  try {
    const controller = new AbortController();
    const timeoutId = setTimeout(() => controller.abort(), 2000);

    const response = await fetch(`${ollamaHost}/api/tags`, {
      signal: controller.signal
    });
    clearTimeout(timeoutId);

    if (response.ok) {
      const data = await response.json();
      res.json({
        available: true,
        host: ollamaHost,
        models: (data as any).models || []
      });
    } else {
      res.json({
        available: false,
        host: ollamaHost,
        status: response.status,
        message: 'Ollama responded with non-200 status'
      });
    }
  } catch (err: any) {
    res.json({
      available: false,
      host: ollamaHost,
      message: err.name === 'AbortError' ? 'Ollama connection timed out' : 'Ollama not detected on host'
    });
  }
});

// 4. Ollama Generation Proxy
app.post('/api/providers/ollama/generate', async (req: Request, res: Response) => {
  try {
    const response = await fetch(`${ollamaHost}/api/generate`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(req.body)
    });

    if (!response.ok) {
      const text = await response.text();
      return res.status(response.status).send(text);
    }

    const data = await response.json();
    res.json(data);
  } catch (error: any) {
    res.status(502).json({ error: error.message || 'Failed to connect to local Ollama instance' });
  }
});

// -------------------------------------------------------------
// VITE CLIENT MOUNTING (Dev & Prod Multi-Platform Delivery)
// -------------------------------------------------------------
async function startServer() {
  const server = http.createServer(app);

  if (!isProduction) {
    // Development mode: Mount Vite middleware with HMR disabled to prevent WebSocket loops on Cloud Run
    const { createServer: createViteServer } = await import('vite');
    const vite = await createViteServer({
      server: {
        middlewareMode: true,
        hmr: false,
      },
      appType: 'spa'
    });
    app.use(vite.middlewares);
  } else {
    // Production mode: Serve built static files
    const distPath = path.resolve(__dirname, 'dist');
    app.use(express.static(distPath));
    app.get('*', (_req: Request, res: Response) => {
      res.sendFile(path.resolve(distPath, 'index.html'));
    });
  }

  server.listen(port, host, () => {
    const addresses = getNetworkInterfaces();
    console.log(`\n======================================================`);
    console.log(`⚓ Fleet AI Core Daemon is running`);
    console.log(`   Host: ${host} | Port: ${port} | Mode: ${isProduction ? 'Production' : 'Development'}`);
    console.log(`------------------------------------------------------`);
    console.log(`📡 Local Access:      http://localhost:${port}`);
    addresses
      .filter((a) => a.address !== '127.0.0.1')
      .forEach((a) => {
        console.log(`📱 LAN / Mobile Access: ${a.url} (${a.name})`);
      });
    console.log(`🌐 Ready for Nginx / Cloudflare / Docker / Webview`);
    console.log(`======================================================\n`);
  });
}

startServer().catch((err) => {
  console.error('Fatal error starting Fleet AI Daemon:', err);
  process.exit(1);
});
