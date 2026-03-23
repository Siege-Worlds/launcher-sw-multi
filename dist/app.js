// ─── Tauri API (loaded after Tauri injects __TAURI__) ───────────────────────

let invoke, listen;

function tauriReady() {
  return !!(window.__TAURI__ && window.__TAURI__.core);
}

function ensureTauri() {
  if (!tauriReady()) {
    log('Tauri API not available yet.');
    return false;
  }
  if (!invoke) {
    invoke = window.__TAURI__.core.invoke;
    listen = window.__TAURI__.event.listen;
    setupTauriListeners();
  }
  return true;
}

// ─── DOM Elements ───────────────────────────────────────────────────────────

const installPathInput = document.getElementById('install-path');
const btnBrowse = document.getElementById('btn-browse');
const btnCheck = document.getElementById('btn-check');
const btnPlay = document.getElementById('btn-play');
const btnSignin = document.getElementById('btn-signin');
const btnSignout = document.getElementById('btn-signout');
const btnSettings = document.getElementById('btn-settings');
const btnSettingsSave = document.getElementById('btn-settings-save');
const btnSettingsCancel = document.getElementById('btn-settings-cancel');
const signedOutArea = document.getElementById('signed-out');
const signedInArea = document.getElementById('signed-in');
const userCic = document.getElementById('user-cic');
const userName = document.getElementById('user-name');
const progressContainer = document.getElementById('progress-container');
const progressFill = document.getElementById('progress-fill');
const progressText = document.getElementById('progress-text');
const consoleLog = document.getElementById('console-log');
const settingsPanel = document.getElementById('settings-panel');

// ─── Slideshow ──────────────────────────────────────────────────────────────

let currentSlide = 0;
const slides = document.querySelectorAll('.slide');

function nextSlide() {
  if (slides.length === 0) return;
  slides[currentSlide].classList.remove('active');
  currentSlide = (currentSlide + 1) % slides.length;
  slides[currentSlide].classList.add('active');
}

setInterval(nextSlide, 5000);

// ─── Console Logging ────────────────────────────────────────────────────────

function log(message) {
  const line = document.createElement('div');
  line.className = 'log-line';
  line.textContent = message;
  consoleLog.appendChild(line);
  consoleLog.parentElement.scrollTop = consoleLog.parentElement.scrollHeight;
}

// ─── Init ───────────────────────────────────────────────────────────────────

async function init() {
  if (!ensureTauri()) return;

  // Load settings
  try {
    const settings = await invoke('get_settings');
    installPathInput.value = settings.install_path;
  } catch (e) {
    log('Failed to load settings: ' + e);
  }

  // Check for stored auth (auto-login)
  try {
    const auth = await invoke('get_stored_auth');
    if (auth.logged_in && auth.user) {
      showUser(auth.user);
      log('Signed in as ' + auth.user.display_name);
    }
  } catch (e) {
    // Not logged in, that's fine
  }

  log('Launcher ready.');
}

// Wait for Tauri to be injected, then init
function waitForTauri() {
  if (tauriReady()) {
    init();
  } else {
    log('Waiting for Tauri...');
    let attempts = 0;
    const interval = setInterval(() => {
      attempts++;
      if (tauriReady()) {
        clearInterval(interval);
        init();
      } else if (attempts > 50) {
        clearInterval(interval);
        log('Tauri API not found. Running in standalone mode.');
      }
    }, 100);
  }
}

// ─── User Display ───────────────────────────────────────────────────────────

function showUser(user) {
  signedOutArea.style.display = 'none';
  signedInArea.style.display = 'flex';
  userName.textContent = user.display_name;

  // Render CIC (Character Identity Circle)
  const size = 40;
  const ringWidth = Math.max(2, Math.round(size * 0.03));
  const gapWidth = Math.max(1, Math.round(size * 0.01));

  userCic.innerHTML = '';
  userCic.style.width = size + 'px';
  userCic.style.height = size + 'px';
  userCic.style.borderRadius = '50%';
  userCic.style.border = ringWidth + 'px solid ' + user.avatar_outer_color;
  userCic.style.padding = gapWidth + 'px';
  userCic.style.background = '#000';
  userCic.style.overflow = 'hidden';

  const inner = document.createElement('div');
  inner.style.width = '100%';
  inner.style.height = '100%';
  inner.style.borderRadius = '50%';
  inner.style.border = ringWidth + 'px solid ' + user.avatar_inner_color;
  inner.style.overflow = 'hidden';

  if (user.avatar_url) {
    const img = document.createElement('img');
    img.src = user.avatar_url;
    img.style.width = '100%';
    img.style.height = '100%';
    img.style.objectFit = 'cover';
    const panX = ((user.avatar_pan_x || 0.5) - 0.5) * -100;
    const panY = ((user.avatar_pan_y || 0.5) - 0.5) * -100;
    const zoom = user.avatar_zoom || 1.0;
    img.style.transform = `scale(${zoom}) translate(${panX}%, ${panY}%)`;
    inner.appendChild(img);
  }

  userCic.appendChild(inner);
}

function showSignedOut() {
  signedOutArea.style.display = 'flex';
  signedInArea.style.display = 'none';
  userName.textContent = '';
  userCic.innerHTML = '';
}

// ─── Event Listeners ────────────────────────────────────────────────────────

btnBrowse.addEventListener('click', async () => {
  if (!ensureTauri()) return;
  try {
    const path = await invoke('select_install_path');
    installPathInput.value = path;
    log('Install path set to: ' + path);
  } catch (e) {
    // User cancelled
  }
});

btnCheck.addEventListener('click', async () => {
  if (!ensureTauri()) return;
  btnCheck.disabled = true;
  log('Checking for updates...');
  try {
    const result = await invoke('check_updates');
    log(result);
    if (result.includes('need updating')) {
      log('Click PLAY to download updates and launch.');
    }
  } catch (e) {
    log('Error: ' + e);
  }
  btnCheck.disabled = false;
});

btnPlay.addEventListener('click', async () => {
  if (!ensureTauri()) return;
  btnPlay.disabled = true;
  btnPlay.textContent = 'UPDATING...';
  progressContainer.style.display = 'flex';

  try {
    await invoke('download_game');
    log('Launching game...');
    await invoke('launch_game');
  } catch (e) {
    log('Error: ' + e);
  }

  progressContainer.style.display = 'none';
  btnPlay.disabled = false;
  btnPlay.textContent = 'PLAY';
});

btnSignin.addEventListener('click', async () => {
  if (!ensureTauri()) return;
  btnSignin.disabled = true;
  btnSignin.textContent = 'Signing in...';
  try {
    const auth = await invoke('start_sso_login');
    if (auth.logged_in && auth.user) {
      showUser(auth.user);
    }
  } catch (e) {
    log('Sign in failed: ' + e);
  }
  btnSignin.disabled = false;
  btnSignin.textContent = 'Sign In';
});

btnSignout.addEventListener('click', async () => {
  if (!ensureTauri()) return;
  try {
    await invoke('logout');
    showSignedOut();
    log('Signed out.');
  } catch (e) {
    log('Error signing out: ' + e);
  }
});

// ─── Settings ───────────────────────────────────────────────────────────────

btnSettings.addEventListener('click', async () => {
  if (!ensureTauri()) return;
  try {
    const settings = await invoke('get_settings');
    document.getElementById('set-build-url').value = settings.build_server_url;
    document.getElementById('set-sso-url').value = settings.sso_url;
    document.getElementById('set-signing-identity').value = settings.signing_identity;
    document.getElementById('set-apple-team').value = settings.apple_team_id;
    document.getElementById('set-win-cert').value = settings.windows_cert_path;
  } catch (e) {
    log('Failed to load settings: ' + e);
  }
  settingsPanel.style.display = 'flex';
});

btnSettingsSave.addEventListener('click', async () => {
  if (!ensureTauri()) return;
  const settings = {
    install_path: installPathInput.value,
    build_server_url: document.getElementById('set-build-url').value,
    sso_url: document.getElementById('set-sso-url').value,
    signing_identity: document.getElementById('set-signing-identity').value,
    apple_team_id: document.getElementById('set-apple-team').value,
    windows_cert_path: document.getElementById('set-win-cert').value,
  };
  try {
    await invoke('save_settings', { settings });
    log('Settings saved.');
    settingsPanel.style.display = 'none';
  } catch (e) {
    log('Failed to save settings: ' + e);
  }
});

btnSettingsCancel.addEventListener('click', () => {
  settingsPanel.style.display = 'none';
});

// ─── Tauri Event Listeners ──────────────────────────────────────────────────

function setupTauriListeners() {
  listen('log', (event) => {
    log(event.payload);
  });

  listen('download-progress', (event) => {
    const { current, total, file } = event.payload;
    const pct = Math.round((current / total) * 100);
    progressFill.style.width = pct + '%';
    progressText.textContent = pct + '%';
    progressContainer.style.display = 'flex';

    if (current >= total) {
      setTimeout(() => {
        progressContainer.style.display = 'none';
      }, 2000);
    }
  });
}

// ─── Start ──────────────────────────────────────────────────────────────────

waitForTauri();
