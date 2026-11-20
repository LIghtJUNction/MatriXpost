#!/usr/bin/env node
import { open, constants } from 'node:fs/promises';
import { resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { randomBytes } from 'node:crypto';
import { createBridge } from './server.mjs';
import { runMcp } from './mcp.mjs';

const HELP = `MatriXpost local AI bridge (Node.js 22+)\n\n  node bridge/cli.mjs serve --extension-id <ID> [--port 43176] [--token-file <private-file>]\n  node bridge/cli.mjs mcp [--port 43176] [--token-file <private-file>]\n\nCopy the extension ID from chrome://extensions. Serve prints the pairing token\nto stderr; paste it into MatriXpost's AI bridge settings. A private token file is\ncreated if missing and safely reused on restart. MCP reads that token from\nMATRIXPOST_TOKEN or the private file. Keep the extension workspace open.\nThe companion accepts only 127.0.0.1 and the explicitly paired extension origin.\n`;

function parseArgs(argv) {
  if (argv.length === 0 || argv[0] === '--help' || argv[0] === '-h') return { mode: 'help' };
  const [mode, ...args] = argv;
  if (!['serve', 'mcp'].includes(mode)) throw new Error('Expected serve or mcp. Use --help for usage.');
  const options = { mode, port: 43176 };
  for (let index = 0; index < args.length; index += 2) {
    const flag = args[index];
    const value = args[index + 1];
    if (!['--port', '--extension-id', '--token-file'].includes(flag) || !value || value.startsWith('--')) throw new Error('Invalid CLI option. Use --help for usage.');
    const key = { '--port': 'port', '--extension-id': 'extensionId', '--token-file': 'tokenFile' }[flag];
    if (key !== 'port' && Object.hasOwn(options, key)) throw new Error('Duplicate CLI option.');
    if (key === 'port') {
      if (!/^\d+$/.test(value) || Number(value) < 1 || Number(value) > 65535) throw new Error('Port must be an integer between 1 and 65535.');
      options.port = Number(value);
    } else options[key] = value;
  }
  if (mode === 'mcp' && options.extensionId) throw new Error('--extension-id applies only to serve.');
  return options;
}

export async function readPrivateToken(path) {
  const file = await open(path, constants.O_RDONLY | (constants.O_NOFOLLOW ?? 0));
  try {
    const stat = await file.stat();
    if (!stat.isFile() || stat.size > 1024 || (process.platform !== 'win32' && ((stat.mode & 0o077) !== 0 || stat.uid !== process.getuid()))) {
      throw new Error('Token file must be a private regular file (chmod 600).');
    }
    const token = (await file.readFile('utf8')).trim();
    if (!/^[a-zA-Z0-9_-]{43,128}$/.test(token)) throw new Error('Token file does not contain a valid pairing token.');
    return token;
  } finally { await file.close(); }
}

export async function loadOrCreatePrivateToken(path) {
  let file;
  try { file = await open(path, 'wx', 0o600); }
  catch (error) {
    if (error.code !== 'EEXIST') throw error;
    // An existing file must pass the same no-symlink, owner and mode checks as MCP.
    return { token: await readPrivateToken(path), created: false };
  }
  const token = randomBytes(32).toString('hex');
  try { await file.writeFile(`${token}\n`, 'utf8'); await file.sync(); }
  finally { await file.close(); }
  return { token, created: true };
}

export async function main(argv = process.argv.slice(2)) {
  const options = parseArgs(argv);
  if (options.mode === 'help') { process.stderr.write(HELP); return; }
  if (options.mode === 'mcp') {
    const token = options.tokenFile ? await readPrivateToken(options.tokenFile) : process.env.MATRIXPOST_TOKEN;
    await runMcp({ token, port: options.port });
    return;
  }
  if (!/^[a-p]{32}$/.test(options.extensionId ?? '')) throw new Error('An exact Chrome extension ID (32 a-p characters) is required.');
  const pairing = options.tokenFile ? await loadOrCreatePrivateToken(options.tokenFile) : null;
  const bridge = createBridge({ extensionId: options.extensionId, ...(pairing ? { token: pairing.token } : {}) });
  const url = await bridge.listen(options.port);
  process.stderr.write(`MatriXpost bridge: ${url}\nExtension: ${options.extensionId}\nPairing token: ${bridge.token}\n${pairing && !pairing.created ? 'Reused the existing private pairing token.\n' : ''}`);
  let stopping = false;
  const stop = async () => {
    if (stopping) return;
    stopping = true;
    await bridge.close();
  };
  process.once('SIGINT', stop);
  process.once('SIGTERM', stop);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((error) => {
    // Configuration errors are controlled messages; never log request or token contents.
    const allowed = /^(?:Expected |Invalid CLI|Duplicate CLI|Port must|--extension-id|Token file|Set MATRIXPOST_TOKEN|An exact Chrome|Could not create)/;
    process.stderr.write(`MatriXpost: ${allowed.test(error.message) ? error.message : 'Unable to start. Check the port, extension ID, and private token file.'}\n`);
    process.exitCode = 1;
  });
}
