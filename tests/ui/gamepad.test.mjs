import test from 'node:test';
import assert from 'node:assert/strict';
import {
  detectControllerType,
  getButtonPrompts,
  getActionHints,
  getGeometricCandidates,
  cycleIndex,
  GamepadManager,
  CONTROLLER_STYLES,
} from '../../ui/gamepad.mjs';

test('detectControllerType correctly identifies controller families', () => {
  assert.equal(detectControllerType('Wireless Controller (STANDARD GAMEPAD Vendor: 054c Product: 0ce6)'), CONTROLLER_STYLES.PLAYSTATION);
  assert.equal(detectControllerType('DualSense Wireless Controller'), CONTROLLER_STYLES.PLAYSTATION);
  assert.equal(detectControllerType('Sony Interactive Entertainment DualShock 4 USB Wireless Adaptor'), CONTROLLER_STYLES.PLAYSTATION);

  assert.equal(detectControllerType('Nintendo Switch Pro Controller (Vendor: 057e Product: 2009)'), CONTROLLER_STYLES.NINTENDO);
  assert.equal(detectControllerType('Joy-Con (L/R)'), CONTROLLER_STYLES.NINTENDO);

  assert.equal(detectControllerType('Xbox 360 Controller (XInput STANDARD GAMEPAD)'), CONTROLLER_STYLES.XBOX);
  assert.equal(detectControllerType('Xbox Wireless Controller'), CONTROLLER_STYLES.XBOX);
  assert.equal(detectControllerType('Generic USB Joystick'), CONTROLLER_STYLES.XBOX);

  // Forced preferences take precedence
  assert.equal(detectControllerType('DualSense', 'xbox'), CONTROLLER_STYLES.XBOX);
  assert.equal(detectControllerType('Xbox Controller', 'playstation'), CONTROLLER_STYLES.PLAYSTATION);
  assert.equal(detectControllerType('Xbox Controller', 'nintendo'), CONTROLLER_STYLES.NINTENDO);
});

test('getButtonPrompts returns appropriate symbols for each style', () => {
  const xbox = getButtonPrompts(CONTROLLER_STYLES.XBOX);
  assert.equal(xbox.a.symbol, 'A');
  assert.equal(xbox.b.symbol, 'B');
  assert.equal(xbox.x.symbol, 'X');
  assert.equal(xbox.y.symbol, 'Y');
  assert.equal(xbox.lb, 'LB');
  assert.equal(xbox.rb, 'RB');
  assert.equal(xbox.lt, 'LT');
  assert.equal(xbox.rt, 'RT');
  assert.equal(xbox.menu, 'Menu');

  const ps = getButtonPrompts(CONTROLLER_STYLES.PLAYSTATION);
  assert.equal(ps.a.symbol, '✕');
  assert.equal(ps.b.symbol, '○');
  assert.equal(ps.x.symbol, '□');
  assert.equal(ps.y.symbol, '△');
  assert.equal(ps.lb, 'L1');
  assert.equal(ps.rb, 'R1');
  assert.equal(ps.lt, 'L2');
  assert.equal(ps.rt, 'R2');
  assert.equal(ps.menu, 'Options');

  const nintendo = getButtonPrompts(CONTROLLER_STYLES.NINTENDO);
  assert.equal(nintendo.a.symbol, 'B');
  assert.equal(nintendo.b.symbol, 'A');
  assert.equal(nintendo.x.symbol, 'Y');
  assert.equal(nintendo.y.symbol, 'X');
  assert.equal(nintendo.lb, 'L');
  assert.equal(nintendo.rb, 'R');
  assert.equal(nintendo.lt, 'ZL');
  assert.equal(nintendo.rt, 'ZR');
  assert.equal(nintendo.menu, '+');
});

test('getActionHints provides context-sensitive prompts', () => {
  const prompts = getButtonPrompts(CONTROLLER_STYLES.XBOX);

  // Modal context
  const modalHints = getActionHints(prompts, { isModal: true });
  assert.equal(modalHints.length, 2);
  assert.equal(modalHints[0].label, 'Выбрать');
  assert.equal(modalHints[1].label, 'Закрыть');

  // Search context
  const searchHints = getActionHints(prompts, { isSearch: true });
  assert.equal(searchHints.length, 1);
  assert.equal(searchHints[0].label, 'Очистить / Выйти');

  // Game card context
  const cardHints = getActionHints(prompts, { isCard: true });
  assert.ok(cardHints.some(h => h.label === 'Запустить'));
  assert.ok(cardHints.some(h => h.label === 'В избранное'));
  assert.ok(cardHints.some(h => h.label === 'Поиск'));
  assert.ok(cardHints.some(h => h.label === 'Вкладки'));
  assert.ok(cardHints.some(h => h.label === 'Платформы'));
  assert.ok(cardHints.some(h => h.label === 'Настройки'));

  // Detail view with back button
  const backHints = getActionHints(prompts, { hasBack: true });
  assert.ok(backHints.some(h => h.label === 'Назад'));
});

test('getGeometricCandidates calculates directional nearest neighbors', () => {
  // 3x3 grid of elements (each 100x100 spaced with 20px gap)
  // [0: 0,0]   [1: 120,0]   [2: 240,0]
  // [3: 0,120] [4: 120,120] [5: 240,120]
  // [6: 0,240] [7: 120,240] [8: 240,240]
  const rects = [];
  for (let row = 0; row < 3; row++) {
    for (let col = 0; col < 3; col++) {
      rects.push({
        left: col * 120,
        top: row * 120,
        width: 100,
        height: 100,
      });
    }
  }

  const centerRect = rects[4]; // Center element (index 4)

  assert.equal(getGeometricCandidates(centerRect, rects, 'right'), 5);
  assert.equal(getGeometricCandidates(centerRect, rects, 'left'), 3);
  assert.equal(getGeometricCandidates(centerRect, rects, 'up'), 1);
  assert.equal(getGeometricCandidates(centerRect, rects, 'down'), 7);

  // Corner element (top-left index 0)
  assert.equal(getGeometricCandidates(rects[0], rects, 'left'), -1); // Nothing left
  assert.equal(getGeometricCandidates(rects[0], rects, 'up'), -1);   // Nothing up
  assert.equal(getGeometricCandidates(rects[0], rects, 'right'), 1);
  assert.equal(getGeometricCandidates(rects[0], rects, 'down'), 3);
});

test('cycleIndex handles circular wrapping', () => {
  assert.equal(cycleIndex(0, 1, 5), 1);
  assert.equal(cycleIndex(4, 1, 5), 0);
  assert.equal(cycleIndex(0, -1, 5), 4);
  assert.equal(cycleIndex(2, -1, 5), 1);
  assert.equal(cycleIndex(0, 0, 0), 0);
  assert.equal(cycleIndex(3, -2, 5), 1);
  assert.equal(cycleIndex(1, -3, 5), 3);
});

test('GamepadManager cleans gamepad names cleanly', () => {
  const gm = new GamepadManager();
  assert.equal(gm.cleanGamepadName('Xbox 360 Controller (XInput STANDARD GAMEPAD)'), 'Xbox 360 Controller');
  assert.equal(gm.cleanGamepadName('Wireless Controller (STANDARD GAMEPAD)'), 'Wireless Controller');
  assert.equal(gm.cleanGamepadName('DualSense Wireless Controller (STANDARD GAMEPAD Vendor: 054c Product: 0ce6)'), 'DualSense Wireless Controller');
  assert.equal(gm.cleanGamepadName(''), 'Геймпад');
  gm.destroy();
});

test('GamepadManager dispatches callbacks on layout and settings actions', () => {
  let settingsToggled = false;
  let layoutToggled = false;

  const gm = new GamepadManager({
    onSettingsToggle: () => { settingsToggled = true; },
    onLayoutToggle: () => { layoutToggled = true; },
  });

  gm.handleButtonPress(8, {}); // View / Select
  assert.equal(layoutToggled, true);

  gm.handleButtonPress(9, {}); // Menu / Start
  assert.equal(settingsToggled, true);

  gm.destroy();
});
