// SWCH Gamepad & Controller Navigation Engine
// Provides full gamepad / controller support for desktop, couch, handhelds (Steam Deck / ROG Ally) and Big Picture mode.

export const CONTROLLER_STYLES = {
  XBOX: 'xbox',
  PLAYSTATION: 'playstation',
  NINTENDO: 'nintendo',
  AUTO: 'auto',
};

const DEFAULT_DEADZONE = 0.38;
const INITIAL_REPEAT_DELAY = 280;
const REPEAT_INTERVAL = 130;

export function detectControllerType(id = '', preferred = 'auto') {
  if (preferred && preferred !== 'auto') {
    return preferred;
  }
  const clean = String(id).toLowerCase();
  if (/(sony|dualsense|dualshock|playstation|ps5|ps4|ps3|054c)/i.test(clean)) {
    return CONTROLLER_STYLES.PLAYSTATION;
  }
  if (/(nintendo|switch|joy-con|pro controller|057e)/i.test(clean)) {
    return CONTROLLER_STYLES.NINTENDO;
  }
  return CONTROLLER_STYLES.XBOX;
}

export function getButtonPrompts(style = CONTROLLER_STYLES.XBOX) {
  switch (style) {
    case CONTROLLER_STYLES.PLAYSTATION:
      return {
        a: { symbol: '✕', text: '✕', name: 'Крест', class: 'gp-ps-cross' },
        b: { symbol: '○', text: '○', name: 'Круг', class: 'gp-ps-circle' },
        x: { symbol: '□', text: '□', name: 'Квадрат', class: 'gp-ps-square' },
        y: { symbol: '△', text: '△', name: 'Треугольник', class: 'gp-ps-triangle' },
        lb: 'L1',
        rb: 'R1',
        lt: 'L2',
        rt: 'R2',
        view: 'Share',
        menu: 'Options',
      };
    case CONTROLLER_STYLES.NINTENDO:
      return {
        a: { symbol: 'B', text: 'B', name: 'B', class: 'gp-nintendo-b' },
        b: { symbol: 'A', text: 'A', name: 'A', class: 'gp-nintendo-a' },
        x: { symbol: 'Y', text: 'Y', name: 'Y', class: 'gp-nintendo-y' },
        y: { symbol: 'X', text: 'X', name: 'X', class: 'gp-nintendo-x' },
        lb: 'L',
        rb: 'R',
        lt: 'ZL',
        rt: 'ZR',
        view: '−',
        menu: '+',
      };
    case CONTROLLER_STYLES.XBOX:
    default:
      return {
        a: { symbol: 'A', text: 'A', name: 'A', class: 'gp-xbox-a' },
        b: { symbol: 'B', text: 'B', name: 'B', class: 'gp-xbox-b' },
        x: { symbol: 'X', text: 'X', name: 'X', class: 'gp-xbox-x' },
        y: { symbol: 'Y', text: 'Y', name: 'Y', class: 'gp-xbox-y' },
        lb: 'LB',
        rb: 'RB',
        lt: 'LT',
        rt: 'RT',
        view: 'View',
        menu: 'Menu',
      };
  }
}

export function getActionHints(prompts, context = {}) {
  const { isModal = false, isSearch = false, isCard = false, hasBack = false } = context;

  if (isModal) {
    return [
      { key: 'a', btn: prompts.a, label: 'Выбрать' },
      { key: 'b', btn: prompts.b, label: 'Закрыть' },
    ];
  }

  if (isSearch) {
    return [
      { key: 'b', btn: prompts.b, label: 'Очистить / Выйти' },
    ];
  }

  const hints = [
    { key: 'a', btn: prompts.a, label: isCard ? 'Запустить' : 'Выбрать' },
  ];

  if (hasBack) {
    hints.push({ key: 'b', btn: prompts.b, label: 'Назад' });
  }

  if (isCard) {
    hints.push({ key: 'x', btn: prompts.x, label: 'В избранное' });
  }

  hints.push(
    { key: 'y', btn: prompts.y, label: 'Поиск' },
    { key: 'lb-rb', btnText: `${prompts.lb}/${prompts.rb}`, label: 'Вкладки' },
    { key: 'lt-rt', btnText: `${prompts.lt}/${prompts.rt}`, label: 'Платформы' },
    { key: 'menu', btnText: prompts.menu, label: 'Настройки' }
  );

  return hints;
}

export function getGeometricCandidates(currentRect, candidatesRects, direction) {
  const currentCenterX = currentRect.left + currentRect.width / 2;
  const currentCenterY = currentRect.top + currentRect.height / 2;

  let bestIndex = -1;
  let bestScore = Infinity;

  for (let i = 0; i < candidatesRects.length; i++) {
    const rect = candidatesRects[i];
    const candidateCenterX = rect.left + rect.width / 2;
    const candidateCenterY = rect.top + rect.height / 2;

    const dx = candidateCenterX - currentCenterX;
    const dy = candidateCenterY - currentCenterY;

    let isCandidate = false;
    let primaryDist = 0;
    let secondaryDist = 0;

    switch (direction) {
      case 'left':
        if (candidateCenterX < currentCenterX - 4) {
          isCandidate = true;
          primaryDist = -dx;
          secondaryDist = Math.abs(dy);
        }
        break;
      case 'right':
        if (candidateCenterX > currentCenterX + 4) {
          isCandidate = true;
          primaryDist = dx;
          secondaryDist = Math.abs(dy);
        }
        break;
      case 'up':
        if (candidateCenterY < currentCenterY - 4) {
          isCandidate = true;
          primaryDist = -dy;
          secondaryDist = Math.abs(dx);
        }
        break;
      case 'down':
        if (candidateCenterY > currentCenterY + 4) {
          isCandidate = true;
          primaryDist = dy;
          secondaryDist = Math.abs(dx);
        }
        break;
    }

    if (isCandidate) {
      // Weight primary distance and penalize orthogonal deviation
      const score = primaryDist + secondaryDist * 2.2;
      if (score < bestScore) {
        bestScore = score;
        bestIndex = i;
      }
    }
  }

  return bestIndex;
}

export function cycleIndex(currentIndex, delta, total) {
  if (total <= 0) return 0;
  return (currentIndex + delta + total) % total;
}

export class GamepadManager {
  constructor(options = {}) {
    this.options = {
      onPageChange: options.onPageChange || (() => {}),
      onPlatformChange: options.onPlatformChange || (() => {}),
      onSettingsToggle: options.onSettingsToggle || (() => {}),
      onLayoutToggle: options.onLayoutToggle || (() => {}),
      toast: options.toast || (() => {}),
      ...options,
    };

    this.enabled = this.readStorage('swch-gamepad-enabled', true);
    this.vibration = this.readStorage('swch-gamepad-vibration', true);
    this.layoutPreference = this.readStorage('swch-gamepad-layout', 'auto');

    this.isActive = false;
    this.activeGamepadIndex = null;
    this.activeGamepadName = '';
    this.detectedStyle = CONTROLLER_STYLES.XBOX;

    this.buttonStates = new Map();
    this.axisState = {
      direction: null,
      firstTrigger: 0,
      lastRepeat: 0,
    };

    this.focusedElement = null;
    this.pollRaf = null;
    this.connectedGamepads = [];

    this.init();
  }

  readStorage(key, fallback) {
    try {
      const val = localStorage.getItem(key);
      return val !== null ? JSON.parse(val) : fallback;
    } catch {
      return fallback;
    }
  }

  saveStorage(key, value) {
    try {
      localStorage.setItem(key, JSON.stringify(value));
    } catch {}
  }

  init() {
    if (typeof window === 'undefined') return;

    window.addEventListener('gamepadconnected', (e) => this.onGamepadConnected(e));
    window.addEventListener('gamepaddisconnected', (e) => this.onGamepadDisconnected(e));

    // When mouse or keyboard is used, soft-deactivate gamepad visual mode
    window.addEventListener('mousemove', (e) => {
      if (Math.abs(e.movementX) > 2 || Math.abs(e.movementY) > 2) {
        this.deactivateGamepadMode();
      }
    }, { passive: true });

    window.addEventListener('keydown', (e) => {
      if (!['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight', 'Tab', 'Enter', 'Escape'].includes(e.key)) {
        this.deactivateGamepadMode();
      }
    }, { passive: true });

    this.startPolling();
  }

  onGamepadConnected(event) {
    const gp = event.gamepad;
    if (!gp) return;
    this.updateConnectedList();
    this.activeGamepadIndex = gp.index;
    this.activeGamepadName = gp.id;
    this.detectedStyle = detectControllerType(gp.id, this.layoutPreference);

    this.options.toast(`🎮 Подключен геймпад: ${this.cleanGamepadName(gp.id)}`);
    this.vibrate('confirm');
    this.updateFooterIndicator();
    this.renderSettingsDevices();
  }

  onGamepadDisconnected(event) {
    const gp = event.gamepad;
    this.updateConnectedList();
    if (this.activeGamepadIndex === gp?.index) {
      this.activeGamepadIndex = null;
      this.activeGamepadName = '';
      this.deactivateGamepadMode();
    }
    this.options.toast('Геймпад отключен');
    this.updateFooterIndicator();
    this.renderSettingsDevices();
  }

  cleanGamepadName(raw = '') {
    return raw
      .replace(/\s*\([^)]*\)/g, '')
      .replace(/standard gamepad/gi, '')
      .replace(/xinput/gi, '')
      .trim() || 'Геймпад';
  }

  updateConnectedList() {
    if (typeof navigator === 'undefined' || !navigator.getGamepads) return;
    const pads = navigator.getGamepads();
    this.connectedGamepads = [];
    for (let i = 0; i < pads.length; i++) {
      if (pads[i] && pads[i].connected) {
        this.connectedGamepads.push(pads[i]);
      }
    }
  }

  startPolling() {
    const poll = () => {
      this.pollInput();
      this.pollRaf = requestAnimationFrame(poll);
    };
    this.pollRaf = requestAnimationFrame(poll);
  }

  pollInput() {
    if (!this.enabled || typeof navigator === 'undefined' || !navigator.getGamepads) return;

    const gamepads = navigator.getGamepads();
    let gp = null;

    if (this.activeGamepadIndex !== null && gamepads[this.activeGamepadIndex]?.connected) {
      gp = gamepads[this.activeGamepadIndex];
    } else {
      for (let i = 0; i < gamepads.length; i++) {
        if (gamepads[i]?.connected) {
          gp = gamepads[i];
          this.activeGamepadIndex = i;
          this.activeGamepadName = gp.id;
          this.detectedStyle = detectControllerType(gp.id, this.layoutPreference);
          break;
        }
      }
    }

    if (!gp) {
      if (this.connectedGamepads.length > 0) {
        this.updateConnectedList();
      }
      return;
    }

    // Update real-time tester UI if visible
    this.updateTesterUI(gp);

    const now = performance.now();

    // 1. Check Buttons
    for (let b = 0; b < gp.buttons.length; b++) {
      const btn = gp.buttons[b];
      const isPressed = btn.pressed || btn.value > 0.5;
      const wasPressed = this.buttonStates.get(b) || false;

      if (isPressed && !wasPressed) {
        this.activateGamepadMode();
        this.handleButtonPress(b, gp);
      }
      this.buttonStates.set(b, isPressed);
    }

    // 2. Check Directional Stick / Axes
    const axisX = gp.axes[0] || 0;
    const axisY = gp.axes[1] || 0;

    let dir = null;
    if (axisX < -DEFAULT_DEADZONE) dir = 'left';
    else if (axisX > DEFAULT_DEADZONE) dir = 'right';
    else if (axisY < -DEFAULT_DEADZONE) dir = 'up';
    else if (axisY > DEFAULT_DEADZONE) dir = 'down';

    // Also check D-Pad buttons if not pressed as discrete button triggers
    if (!dir) {
      if (gp.buttons[12]?.pressed) dir = 'up';
      else if (gp.buttons[13]?.pressed) dir = 'down';
      else if (gp.buttons[14]?.pressed) dir = 'left';
      else if (gp.buttons[15]?.pressed) dir = 'right';
    }

    if (dir) {
      this.activateGamepadMode();
      if (this.axisState.direction !== dir) {
        this.axisState.direction = dir;
        this.axisState.firstTrigger = now;
        this.axisState.lastRepeat = now;
        this.navigate(dir);
      } else {
        const elapsed = now - this.axisState.firstTrigger;
        if (elapsed > INITIAL_REPEAT_DELAY && now - this.axisState.lastRepeat > REPEAT_INTERVAL) {
          this.axisState.lastRepeat = now;
          this.navigate(dir);
        }
      }
    } else {
      this.axisState.direction = null;
    }

    // 3. Right Stick Scrolling
    const rightAxisY = gp.axes[3] || 0;
    if (Math.abs(rightAxisY) > 0.3) {
      const scrollAmount = rightAxisY * 16;
      const scrollContainer = this.findActiveScrollContainer();
      if (scrollContainer) {
        scrollContainer.scrollBy({ top: scrollAmount, behavior: 'auto' });
      } else {
        window.scrollBy({ top: scrollAmount, behavior: 'auto' });
      }
    }
  }

  activateGamepadMode() {
    if (!this.isActive) {
      this.isActive = true;
      document.body.classList.add('gamepad-active');
      this.ensureInitialFocus();
    }
    this.updateHUD();
  }

  deactivateGamepadMode() {
    if (this.isActive) {
      this.isActive = false;
      document.body.classList.remove('gamepad-active');
      if (this.focusedElement) {
        this.focusedElement.classList.remove('gamepad-focused');
      }
    }
  }

  ensureInitialFocus() {
    const modal = document.querySelector('dialog[open]');
    if (modal) {
      const first = modal.querySelector('button:not([disabled]), input:not([disabled]), select:not([disabled])');
      if (first) {
        this.setFocus(first);
        return;
      }
    }

    // If currently focused element is valid, keep it
    if (document.activeElement && document.activeElement !== document.body && document.activeElement.isConnected) {
      this.setFocus(document.activeElement);
      return;
    }

    // Default to first game card, or active nav item
    const firstCard = document.querySelector('.games-grid .game-card, .detail-layout .game-list button');
    if (firstCard) {
      this.setFocus(firstCard);
      return;
    }

    const navItem = document.querySelector('.sidebar .nav-item.active, .sidebar .nav-item');
    if (navItem) {
      this.setFocus(navItem);
    }
  }

  setFocus(el) {
    if (!el) return;
    if (this.focusedElement && this.focusedElement !== el) {
      this.focusedElement.classList.remove('gamepad-focused');
    }
    this.focusedElement = el;
    el.classList.add('gamepad-focused');

    if (typeof el.focus === 'function') {
      try {
        el.focus({ preventScroll: true });
      } catch {}
    }

    el.scrollIntoView({ behavior: 'smooth', block: 'nearest', inline: 'nearest' });
    this.updateHUD();
  }

  getFocusableCandidates() {
    const modal = document.querySelector('dialog[open]');
    if (modal) {
      return Array.from(modal.querySelectorAll(
        'button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex="0"]'
      )).filter(el => el.offsetParent !== null && !el.hidden);
    }

    // Main workspace
    const selectors = [
      '.sidebar .nav-item:not([hidden])',
      '.sidebar .platform-item:not([hidden])',
      '.sidebar .connect-link',
      '.sidebar [data-action="settings"]',
      '.filter-tabs button:not([hidden])',
      '#search-input',
      '#sort-select',
      '.view-switch button',
      '.games-grid .game-card',
      '.detail-layout .game-list button',
      '.detail-panel button:not([disabled])',
      '.empty-actions button',
    ];

    const elements = Array.from(document.querySelectorAll(selectors.join(',')))
      .filter(el => el.offsetParent !== null && !el.hidden && getComputedStyle(el).display !== 'none');

    return elements;
  }

  navigate(direction) {
    if (!this.focusedElement || !this.focusedElement.isConnected) {
      this.ensureInitialFocus();
      return;
    }

    const current = this.focusedElement;

    // Special case for .games-grid
    if (current.matches('.game-card')) {
      const grid = current.closest('.games-grid');
      if (grid) {
        const cards = Array.from(grid.querySelectorAll('.game-card'));
        const index = cards.indexOf(current);

        if (direction === 'left' && index > 0) {
          this.setFocus(cards[index - 1]);
          this.vibrate('click');
          return;
        }
        if (direction === 'right' && index < cards.length - 1) {
          this.setFocus(cards[index + 1]);
          this.vibrate('click');
          return;
        }

        // Left from leftmost column moves to sidebar
        if (direction === 'left' && index % this.estimateGridColumns(grid) === 0) {
          const nav = document.querySelector('.sidebar .nav-item.active') || document.querySelector('.sidebar .nav-item');
          if (nav) {
            this.setFocus(nav);
            this.vibrate('click');
            return;
          }
        }
      }
    }

    // Special case: moving Right from sidebar enters main content / game cards
    if (direction === 'right' && current.closest('.sidebar')) {
      const target = document.querySelector('.games-grid .game-card, .detail-layout .game-list button, .filter-tabs button.active');
      if (target) {
        this.setFocus(target);
        this.vibrate('click');
        return;
      }
    }

    // General geometric spatial navigation
    const candidates = this.getFocusableCandidates();
    if (!candidates.length) return;

    const currentRect = current.getBoundingClientRect();
    const candidateRects = candidates.map(el => el.getBoundingClientRect());

    const nextIndex = getGeometricCandidates(currentRect, candidateRects, direction);
    if (nextIndex !== -1 && candidates[nextIndex] !== current) {
      this.setFocus(candidates[nextIndex]);
      this.vibrate('click');
    }
  }

  estimateGridColumns(grid) {
    const cards = grid.querySelectorAll('.game-card');
    if (cards.length < 2) return 1;
    const firstTop = cards[0].offsetTop;
    let count = 0;
    for (let i = 0; i < cards.length; i++) {
      if (cards[i].offsetTop === firstTop) count++;
      else break;
    }
    return Math.max(1, count);
  }

  findActiveScrollContainer() {
    let el = this.focusedElement;
    while (el && el !== document.body) {
      const overflowY = getComputedStyle(el).overflowY;
      if ((overflowY === 'auto' || overflowY === 'scroll') && el.scrollHeight > el.clientHeight) {
        return el;
      }
      el = el.parentElement;
    }
    return null;
  }

  handleButtonPress(buttonIndex, gamepad) {
    switch (buttonIndex) {
      case 0: // A (Bottom face button)
        this.handleActionA();
        break;
      case 1: // B (Right face button)
        this.handleActionB();
        break;
      case 2: // X (Left face button)
        this.handleActionX();
        break;
      case 3: // Y (Top face button)
        this.handleActionY();
        break;
      case 4: // LB
        this.handleBumper(-1);
        break;
      case 5: // RB
        this.handleBumper(1);
        break;
      case 6: // LT
        this.handleTrigger(-1);
        break;
      case 7: // RT
        this.handleTrigger(1);
        break;
      case 8: // View / Select
        this.options.onLayoutToggle();
        this.vibrate('confirm');
        break;
      case 9: // Menu / Start
        this.options.onSettingsToggle();
        this.vibrate('confirm');
        break;
    }
  }

  handleActionA() {
    const el = this.focusedElement;
    if (!el) return;

    this.vibrate('confirm');

    // If game card: trigger launch or open
    if (el.matches('.game-card')) {
      const playBtn = el.querySelector('.card-play:not([disabled])');
      if (playBtn) {
        playBtn.click();
        return;
      }
      const posterBtn = el.querySelector('.poster-button');
      if (posterBtn) {
        posterBtn.click();
        return;
      }
    }

    // Default: click the element
    if (typeof el.click === 'function') {
      el.click();
    }
  }

  handleActionB() {
    this.vibrate('back');

    // 1. Close open dialog
    const modal = document.querySelector('dialog[open]');
    if (modal) {
      const closeBtn = modal.querySelector('[data-close], [data-action="close"], .icon-button');
      if (closeBtn) closeBtn.click();
      else modal.close();
      return;
    }

    // 2. If search input active: clear and blur
    const search = document.getElementById('search-input');
    if (document.activeElement === search || this.focusedElement === search) {
      if (search.value) {
        search.value = '';
        search.dispatchEvent(new Event('input', { bubbles: true }));
      }
      search.blur();
      this.ensureInitialFocus();
      return;
    }

    // 3. Detail view back button
    const backBtn = document.querySelector('[data-action="back-cards"]');
    if (backBtn && backBtn.offsetParent !== null) {
      backBtn.click();
      return;
    }
  }

  handleActionX() {
    const el = this.focusedElement;
    if (!el) return;

    // Toggle favorite on card or in detail view
    const favBtn = el.matches('.game-card')
      ? el.querySelector('.favorite')
      : el.querySelector('.favorite') || document.querySelector('.detail-panel .favorite');

    if (favBtn) {
      this.vibrate('favorite');
      favBtn.click();
      this.updateHUD();
    }
  }

  handleActionY() {
    this.vibrate('click');
    const search = document.getElementById('search-input');
    if (search) {
      this.setFocus(search);
      search.focus();
      search.select();
    }
  }

  handleBumper(delta) {
    this.vibrate('click');
    const pages = ['library', 'favorites', 'mods', 'inventory', 'accounts'];
    const activeNav = document.querySelector('.sidebar .nav-item.active');
    const currentPage = activeNav?.dataset?.page || 'library';
    const currentIndex = pages.indexOf(currentPage);
    const nextIndex = cycleIndex(currentIndex === -1 ? 0 : currentIndex, delta, pages.length);
    const nextPage = pages[nextIndex];

    const targetBtn = document.querySelector(`.sidebar .nav-item[data-page="${nextPage}"]`);
    if (targetBtn) {
      targetBtn.click();
      setTimeout(() => this.ensureInitialFocus(), 80);
    }
  }

  handleTrigger(delta) {
    this.vibrate('click');
    const platformBtns = Array.from(document.querySelectorAll('.sidebar .platform-item:not([hidden])'))
      .filter(el => el.offsetParent !== null);
    if (!platformBtns.length) return;

    const currentSelected = platformBtns.findIndex(b => b.classList.contains('selected'));
    const nextIndex = cycleIndex(currentSelected === -1 ? 0 : currentSelected, delta, platformBtns.length);

    platformBtns[nextIndex].click();
    setTimeout(() => this.ensureInitialFocus(), 80);
  }

  vibrate(type = 'click') {
    if (!this.vibration || typeof navigator === 'undefined' || !navigator.getGamepads) return;
    const gamepads = navigator.getGamepads();
    const gp = gamepads[this.activeGamepadIndex] || gamepads[0];
    if (!gp || !gp.vibrationActuator) return;

    const patterns = {
      click: { startDelay: 0, duration: 25, weakMagnitude: 0.25, strongMagnitude: 0.1 },
      confirm: { startDelay: 0, duration: 60, weakMagnitude: 0.6, strongMagnitude: 0.3 },
      favorite: { startDelay: 0, duration: 40, weakMagnitude: 0.7, strongMagnitude: 0.2 },
      back: { startDelay: 0, duration: 30, weakMagnitude: 0.3, strongMagnitude: 0.3 },
    };

    const effect = patterns[type] || patterns.click;
    try {
      gp.vibrationActuator.playEffect('dual-rumble', effect).catch(() => {});
    } catch {}
  }

  updateHUD() {
    const hud = document.getElementById('gamepad-hud');
    if (!hud) return;

    if (!this.isActive || !this.enabled) {
      hud.classList.remove('visible');
      return;
    }

    hud.classList.add('visible');

    const style = this.detectedStyle;
    const prompts = getButtonPrompts(style);

    const isModal = Boolean(document.querySelector('dialog[open]'));
    const isSearch = this.focusedElement === document.getElementById('search-input');
    const isCard = Boolean(this.focusedElement?.matches('.game-card'));
    const hasBack = isModal || isSearch || Boolean(document.querySelector('[data-action="back-cards"]')?.offsetParent);

    const hints = getActionHints(prompts, { isModal, isSearch, isCard, hasBack });

    const nameEl = document.getElementById('hud-gamepad-name');
    if (nameEl) {
      nameEl.textContent = this.cleanGamepadName(this.activeGamepadName);
    }

    const actionsContainer = document.getElementById('hud-actions');
    if (actionsContainer) {
      actionsContainer.innerHTML = hints.map(h => {
        const badge = h.btn
          ? `<span class="gp-badge ${h.btn.class}">${h.btn.symbol}</span>`
          : `<span class="gp-badge gp-bumper">${h.btnText}</span>`;
        return `<div class="hud-hint">${badge}<span>${h.label}</span></div>`;
      }).join('');
    }
  }

  updateFooterIndicator() {
    const pill = document.getElementById('footer-gamepad');
    const label = document.getElementById('footer-gamepad-name');
    if (!pill || !label) return;

    if (this.connectedGamepads.length > 0 && this.enabled) {
      pill.hidden = false;
      label.textContent = this.cleanGamepadName(this.connectedGamepads[0].id);
    } else {
      pill.hidden = true;
    }
  }

  updateTesterUI(gp) {
    const testerGrid = document.getElementById('gamepad-tester-grid');
    if (!testerGrid || testerGrid.offsetParent === null) return;

    // Buttons
    const btnNodes = testerGrid.querySelectorAll('[data-tester-btn]');
    btnNodes.forEach(node => {
      const idx = parseInt(node.dataset.testerBtn, 10);
      const pressed = gp.buttons[idx]?.pressed || gp.buttons[idx]?.value > 0.5;
      node.classList.toggle('active', pressed);
    });

    // Sticks
    const leftStickEl = document.getElementById('tester-left-stick');
    if (leftStickEl) {
      const lx = (gp.axes[0] || 0).toFixed(2);
      const ly = (gp.axes[1] || 0).toFixed(2);
      leftStickEl.textContent = `L-Stick: ${lx}, ${ly}`;
    }

    const rightStickEl = document.getElementById('tester-right-stick');
    if (rightStickEl) {
      const rx = (gp.axes[2] || 0).toFixed(2);
      const ry = (gp.axes[3] || 0).toFixed(2);
      rightStickEl.textContent = `R-Stick: ${rx}, ${ry}`;
    }
  }

  renderSettingsDevices() {
    const badge = document.getElementById('gamepad-status-badge');
    const list = document.getElementById('gamepad-devices-list');
    if (!badge || !list) return;

    if (!this.connectedGamepads.length) {
      badge.textContent = 'Не подключен';
      badge.style.background = '#23242a';
      badge.style.color = '#858793';
      list.innerHTML = `<p class="muted small-text" style="line-height:1.6;">Подключите геймпад через USB или Bluetooth. Поддерживаются геймпады Xbox Series/One/360, PlayStation DualSense/DualShock 4, Nintendo Switch Pro Controller, Steam Deck и стандартные USB-контроллеры.</p>`;
      return;
    }

    badge.textContent = `Подключено: ${this.connectedGamepads.length}`;
    badge.style.background = 'rgba(74, 222, 128, 0.2)';
    badge.style.color = '#4ade80';

    list.innerHTML = this.connectedGamepads.map((gp, i) => {
      const type = detectControllerType(gp.id, this.layoutPreference);
      return `
        <div class="gamepad-device-card" style="display:flex; align-items:center; justify-content:space-between; padding:10px 14px; background:rgba(255,255,255,0.04); border:1px solid rgba(255,255,255,0.08); border-radius:8px;">
          <div>
            <div style="display:flex; align-items:center; gap:8px;">
              <span style="font-size:16px;">🎮</span>
              <strong>${this.cleanGamepadName(gp.id)}</strong>
              <span class="badge" style="font-size:10px; padding:2px 6px; border-radius:4px; background:rgba(var(--accent-rgb),0.15); color:var(--accent); text-transform:uppercase;">${type}</span>
            </div>
            <small class="muted" style="font-size:10px; display:block; margin-top:2px;">ID: ${gp.id}</small>
          </div>
          <button type="button" class="secondary mini-btn" data-action="test-vibrate" data-gamepad="${i}">Вибротест</button>
        </div>
      `;
    }).join('');

    list.querySelectorAll('[data-action="test-vibrate"]').forEach(btn => {
      btn.onclick = () => {
        this.vibrate('confirm');
      };
    });
  }

  destroy() {
    if (this.pollRaf) {
      cancelAnimationFrame(this.pollRaf);
      this.pollRaf = null;
    }
    this.deactivateGamepadMode();
  }
}
