const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const {test} = require('node:test');

// Exercise the actual popup builder and payload handler without a running Shell.
// Actor stubs cover data/state behavior; native St rendering remains a manual check.
class Actor {
    constructor(props = {}) {
        Object.assign(this, {visible: true, text: '', children: [], clutter_text: {}, layout: {}}, props);
    }
    add_child(child) { this.children.push(child); }
    set_child(child) { this.children = [child]; }
    get_layout_manager() { return this.layout; }
    connect() {}
    set_style_class_name(value) { this.style_class = value; }
    add_style_class_name(value) { this.style_class = `${this.style_class || ''} ${value}`; }
    remove_style_class_name(value) { this.style_class = (this.style_class || '').replace(value, ''); }
    get_allocation_box() { return {}; }
    get_theme_node() { return {get_content_box: () => ({get_width: () => 360})}; }
}

class FixedDate extends Date {
    constructor(...args) { super(...(args.length ? args : [2026, 8, 9, 10])); }
    static now() { return new FixedDate().getTime(); }
}

const context = vm.createContext({
    Date: FixedDate,
    global: {stage: {}},
    imports: {
        gi: {
            Clutter: {ActorAlign: {CENTER: 1, END: 2, START: 3}},
            Gio: {}, GLib: {},
            GObject: {registerClass: value => value},
            Pango: {EllipsizeMode: {NONE: 0}, WrapMode: {WORD_CHAR: 0}},
            St: {
                BoxLayout: Actor, Bin: Actor, Label: Actor, Widget: Actor,
                ThemeContext: {get_for_stage: () => ({scale_factor: 1})},
            },
        },
        ui: {main: {}, panelMenu: {Button: Actor}, popupMenu: {PopupBaseMenuItem: Actor}},
        misc: {extensionUtils: {getCurrentExtension: () => ({})}},
    },
});
vm.runInContext(fs.readFileSync('extension/extension.js', 'utf8') + '\nthis.Indicator = CreditIndicator;', context);

function popup() {
    const indicator = new context.Indicator();
    indicator.menu = {addMenuItem() {}};
    indicator._panelLabel = new Actor();
    indicator._buildDashboard();
    return indicator;
}

function payload(overrides = {}) {
    return {
        status: 'ok', fresh: true, sample_count: 10,
        current: {credits_used: 22982, plan: 'business'},
        metrics: {rate_per_hour: 44, delta_today: 671, projected_at_reset: 82130},
        daily: Array.from({length: 7}, (_, index) => ({date: `2026-09-0${index + 3}`, credits: index === 6 ? 671 : 0})),
        ...overrides,
    };
}

test('one active day and a zero-usage week both remain visible', () => {
    const view = popup();
    view._applyPayload(payload());
    assert.equal(view._chart.visible, true);
    assert.equal(view._pulseTotal.text, '$6.71');
    assert.equal(view._dailyValues[6].text, '$6.71');
    assert.equal(view._dailyLabels[0].text, 'Thu');
    assert.equal(view._dailyLabels[6].text, 'Today');
    assert.equal(view._dailyBars[6].height, 64);
    view._applyPayload(payload({daily: payload().daily.map(day => ({...day, credits: 0}))}));
    assert.equal(view._chart.visible, true);
    assert.equal(view._pulseTotal.text, '$0.00');
    assert.equal(view._dailyBars[6].height, 1);
});

test('dates determine placement; stale last entry is never relabeled Today', () => {
    const view = popup();
    view._applyPayload(payload({fresh: false, daily: [
        {date: '2026-09-08', credits: 200},
        {date: '2026-08-30', credits: 9999},
        {date: '2026-09-04', credits: 100},
        {date: '2026-09-10', credits: 9999},
    ]}));
    assert.equal(view._pulseTotal.text, '≥ $3.00');
    assert.equal(view._dailyValues[1].text, '$1.00');
    assert.equal(view._dailyValues[5].text, '$2.00');
    assert.equal(view._dailyValues[6].text, '—');
    assert.equal(view._dailyBars[6].visible, false);
    assert.equal(view._status.text, '● Cached');
});

test('missing metrics and empty responses do not become zero or Live', () => {
    const view = popup();
    view._applyPayload({status: 'empty', current: {credits_used: null}, metrics: {rate_per_hour: null}});
    assert.equal(view._panelLabel.text, '—');
    assert.equal(view._used.text, '—');
    assert.equal(view._usedDetail.text, '— AIC');
    assert.equal(view._status.text, '● No data');
    assert.equal(view._chart.visible, false);
    assert.equal(view._pulseTotal.text, '—');
});

test('error recovery clears cached state and allowance uses allocated width', () => {
    const view = popup();
    view._applyPayload(payload({status: 'error', fresh: false, error: 'Offline'}));
    assert.equal(view._error.visible, true);
    assert.equal(view._status.text, '● Cached');
    view._applyPayload(payload({current: {credits_used: 50, entitlement: 100, remaining: 50}}));
    assert.equal(view._error.visible, false);
    assert.equal(view._status.text, '● Live');
    assert.equal(view._progress.width, 180);
    assert.equal(view._progressTrack.visible, true);
    view._applyPayload(payload());
    assert.equal(view._progressTrack.visible, false);
});

test('each chart column contains its own value, bar and label with equal widths', () => {
    const view = popup();
    assert.equal(view._chart.layout.homogeneous, true);
    assert.equal(view._chart.children.length, 7);
    for (const column of view._chart.children)
        assert.equal(column.children.length, 3);
    assert.equal(view._projectionDetail.clutter_text.line_wrap, true);
});

test('calendar slots stay contiguous across DST and month boundaries', () => {
    for (const now of [new Date(2026, 2, 31, 12), new Date(2026, 9, 27, 12), new Date(2026, 0, 2, 12)]) {
        const days = context.chartDays([], now);
        assert.equal(new Set(days.map(day => day.date)).size, 7);
        assert.equal(days.filter(day => day.today).length, 1);
        for (let index = 0; index < 7; index++) {
            const date = new Date(now.getFullYear(), now.getMonth(), now.getDate() - 6 + index);
            assert.equal(days[index].date, context.dateKey(date));
        }
    }
});
