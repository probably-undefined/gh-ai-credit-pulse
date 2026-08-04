/* exported init */

const {Clutter, Gio, GLib, GObject, St} = imports.gi;
const Main = imports.ui.main;
const PanelMenu = imports.ui.panelMenu;
const PopupMenu = imports.ui.popupMenu;
const ExtensionUtils = imports.misc.extensionUtils;

const Me = ExtensionUtils.getCurrentExtension();
const POLL_SECONDS = 30;

const CreditIndicator = GObject.registerClass(
class CreditIndicator extends PanelMenu.Button {
    _init() {
        super._init(0.0, 'GitHub AI Credit Pulse', false);

        this._pollSource = 0;
        this._closeSource = 0;
        this._refreshing = false;
        this._cancellable = new Gio.Cancellable();
        this._collector = GLib.build_filenamev([Me.path, 'scripts', 'gh_ai_credits.py']);
        this.menu.box.add_style_class_name('credit-pulse-menu-content');

        this._panelLabel = new St.Label({
            text: '$—',
            y_align: Clutter.ActorAlign.CENTER,
            style_class: 'credit-pulse-panel-label',
        });
        this.add_child(this._panelLabel);

        this._buildDashboard();

        this.connect('enter-event', () => {
            this._cancelClose();
            this.menu.open();
            return Clutter.EVENT_PROPAGATE;
        });
        this.connect('leave-event', () => {
            this._queueClose();
            return Clutter.EVENT_PROPAGATE;
        });
        this.menu.actor.connect('enter-event', () => {
            this._cancelClose();
            return Clutter.EVENT_PROPAGATE;
        });
        this.menu.actor.connect('leave-event', () => {
            this._queueClose();
            return Clutter.EVENT_PROPAGATE;
        });
        this.menu.connect('open-state-changed', (_menu, open) => {
            if (open)
                this._refresh(true);
        });

        this._pollSource = GLib.timeout_add_seconds(
            GLib.PRIORITY_DEFAULT,
            POLL_SECONDS,
            () => {
                this._refresh(true);
                return GLib.SOURCE_CONTINUE;
            }
        );
        this._refresh(true);
    }

    _buildDashboard() {
        const contentItem = new PopupMenu.PopupBaseMenuItem({
            reactive: false,
            can_focus: false,
            style_class: 'credit-pulse-menu-item',
        });
        const dashboard = new St.BoxLayout({
            vertical: true,
            style_class: 'credit-pulse-dashboard',
        });
        contentItem.add_child(dashboard);

        const header = new St.BoxLayout({style_class: 'credit-pulse-header'});
        header.add_child(new St.Label({
            text: '✦',
            y_align: Clutter.ActorAlign.CENTER,
            style_class: 'credit-pulse-brand-mark',
        }));
        const titleBox = new St.BoxLayout({vertical: true, x_expand: true});
        titleBox.add_child(new St.Label({
            text: 'AI CREDIT PULSE',
            style_class: 'credit-pulse-title',
        }));
        this._subtitle = new St.Label({
            text: 'Loading GitHub usage…',
            style_class: 'credit-pulse-subtitle',
        });
        titleBox.add_child(this._subtitle);
        header.add_child(titleBox);
        this._status = new St.Label({
            text: '● Loading',
            y_align: Clutter.ActorAlign.CENTER,
            style_class: 'credit-pulse-status',
        });
        header.add_child(this._status);
        dashboard.add_child(header);

        const hero = new St.BoxLayout({vertical: true, style_class: 'credit-pulse-hero'});
        const heroHeader = new St.BoxLayout();
        heroHeader.add_child(new St.Label({
            text: 'CURRENT BILLING CYCLE',
            x_expand: true,
            style_class: 'credit-pulse-kicker credit-pulse-kicker-violet',
        }));
        heroHeader.add_child(new St.Label({
            text: '100 AIC = $1',
            style_class: 'credit-pulse-conversion',
        }));
        hero.add_child(heroHeader);
        this._used = new St.Label({text: '$—', style_class: 'credit-pulse-hero-value'});
        this._usedDetail = new St.Label({text: '— AIC', style_class: 'credit-pulse-detail'});
        hero.add_child(this._used);
        hero.add_child(this._usedDetail);
        dashboard.add_child(hero);

        const metrics = new St.BoxLayout({style_class: 'credit-pulse-metrics'});
        [
            ['TODAY', '_today', '_todayDetail'],
            ['6-HOUR RATE', '_rate', '_rateDetail'],
            ['PROJECTION', '_projection', '_projectionDetail'],
        ].forEach(([title, valueName, detailName]) => {
            const card = new St.BoxLayout({vertical: true, x_expand: true, style_class: 'credit-pulse-card'});
            card.add_child(new St.Label({text: title, style_class: 'credit-pulse-kicker'}));
            this[valueName] = new St.Label({text: '—', style_class: 'credit-pulse-card-value'});
            this[detailName] = new St.Label({text: '—', style_class: 'credit-pulse-detail'});
            card.add_child(this[valueName]);
            card.add_child(this[detailName]);
            metrics.add_child(card);
        });
        dashboard.add_child(metrics);

        const pulse = new St.BoxLayout({vertical: true, style_class: 'credit-pulse-pulse'});
        const pulseHeader = new St.BoxLayout();
        pulseHeader.add_child(new St.Label({
            text: 'LAST 7 DAYS',
            x_expand: true,
            style_class: 'credit-pulse-kicker credit-pulse-kicker-cyan',
        }));
        this._pulseTotal = new St.Label({text: '$—', style_class: 'credit-pulse-pulse-total'});
        pulseHeader.add_child(this._pulseTotal);
        pulse.add_child(pulseHeader);
        const chart = new St.BoxLayout({style_class: 'credit-pulse-chart'});
        const labels = new St.BoxLayout({style_class: 'credit-pulse-chart-labels'});
        this._chart = chart;
        this._chartLabels = labels;
        this._dailyBars = [];
        this._dailyLabels = [];
        for (let index = 0; index < 7; index++) {
            const slot = new St.Bin({x_expand: true, y_align: Clutter.ActorAlign.END});
            const bar = new St.Widget({style_class: 'credit-pulse-chart-bar'});
            bar.set_width(34);
            bar.set_height(6);
            slot.set_child(bar);
            chart.add_child(slot);
            const label = new St.Label({text: '·', x_expand: true, style_class: 'credit-pulse-chart-label'});
            labels.add_child(label);
            this._dailyBars.push(bar);
            this._dailyLabels.push(label);
        }
        pulse.add_child(chart);
        pulse.add_child(labels);
        this._pulseEmpty = new St.Label({
            text: 'More than one day of history is needed.',
            visible: false,
            style_class: 'credit-pulse-empty',
        });
        pulse.add_child(this._pulseEmpty);
        dashboard.add_child(pulse);

        const allowance = new St.BoxLayout({vertical: true, style_class: 'credit-pulse-allowance'});
        const allowanceHeader = new St.BoxLayout();
        allowanceHeader.add_child(new St.Label({
            text: 'MONTHLY ALLOWANCE',
            x_expand: true,
            style_class: 'credit-pulse-kicker',
        }));
        this._allowanceText = new St.Label({text: 'Not reported', style_class: 'credit-pulse-detail'});
        allowanceHeader.add_child(this._allowanceText);
        allowance.add_child(allowanceHeader);
        this._progressTrack = new St.Bin({style_class: 'credit-pulse-progress-track'});
        this._progress = new St.Widget({style_class: 'credit-pulse-progress'});
        this._progressTrack.set_child(this._progress);
        allowance.add_child(this._progressTrack);
        this._remaining = new St.Label({text: '— remaining', style_class: 'credit-pulse-detail'});
        allowance.add_child(this._remaining);
        dashboard.add_child(allowance);

        this._error = new St.Label({
            text: '',
            visible: false,
            style_class: 'credit-pulse-error',
        });
        dashboard.add_child(this._error);

        this.menu.addMenuItem(contentItem);
        this.menu.addMenuItem(new PopupMenu.PopupSeparatorMenuItem());

        const refreshItem = new PopupMenu.PopupMenuItem('↻   Refresh now');
        refreshItem.add_style_class_name('credit-pulse-action');
        refreshItem.connect('activate', () => this._refresh(true));
        this.menu.addMenuItem(refreshItem);

        const dashboardItem = new PopupMenu.PopupMenuItem('↗   Open full dashboard');
        dashboardItem.add_style_class_name('credit-pulse-action');
        dashboardItem.connect('activate', () => {
            const launcher = GLib.build_filenamev([GLib.get_home_dir(), '.local', 'bin', 'gh-ai-credit-pulse']);
            try {
                Gio.Subprocess.new([launcher], Gio.SubprocessFlags.NONE);
            } catch (error) {
                this._showError(`Could not open dashboard: ${error.message}`);
            }
        });
        this.menu.addMenuItem(dashboardItem);

        const updateItem = new PopupMenu.PopupMenuItem('↓   Install latest update');
        updateItem.add_style_class_name('credit-pulse-action');
        updateItem.connect('activate', () => {
            const launcher = GLib.build_filenamev([GLib.get_home_dir(), '.local', 'bin', 'gh-ai-credit-pulse']);
            try {
                Gio.Subprocess.new([launcher, '--self-update'], Gio.SubprocessFlags.NONE);
            } catch (error) {
                this._showError(`Could not start updater: ${error.message}`);
            }
        });
        this.menu.addMenuItem(updateItem);
    }

    _queueClose() {
        this._cancelClose();
        this._closeSource = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 240, () => {
            this._closeSource = 0;
            if (!this.hover && !this.menu.actor.hover)
                this.menu.close();
            return GLib.SOURCE_REMOVE;
        });
    }

    _cancelClose() {
        if (this._closeSource) {
            GLib.source_remove(this._closeSource);
            this._closeSource = 0;
        }
    }

    _refresh(fetch) {
        if (this._refreshing)
            return;
        this._refreshing = true;
        this._status.text = '● Syncing';

        let process;
        try {
            process = Gio.Subprocess.new(
                ['/usr/bin/python3', this._collector, fetch ? 'sample' : 'dashboard', '--window', '24h'],
                Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_PIPE
            );
        } catch (error) {
            this._refreshing = false;
            this._showError(error.message);
            return;
        }

        process.communicate_utf8_async(null, this._cancellable, (source, result) => {
            if (this._cancellable.is_cancelled())
                return;
            this._refreshing = false;
            try {
                const [, stdout, stderr] = source.communicate_utf8_finish(result);
                if (!stdout)
                    throw new Error((stderr || 'Collector returned no data').trim());
                const payload = JSON.parse(stdout);
                this._applyPayload(payload);
            } catch (error) {
                this._showError(error.message);
            }
        });
    }

    _applyPayload(payload) {
        const current = payload.current || {};
        const metrics = payload.metrics || {};
        const parsedUsed = Number(current.credits_used);
        const used = Number.isFinite(parsedUsed) ? parsedUsed : null;
        const parsedRate = Number(metrics.rate_per_hour);
        const rate = Number.isFinite(parsedRate) ? parsedRate : null;

        this._panelLabel.text = rate === null
            ? this._money(used)
            : `${this._money(used)} · ${this._money(rate)}/h`;
        this._used.text = this._money(used);
        this._usedDetail.text = `${this._number(used)} AIC`;
        this._today.text = this._money(metrics.delta_today, true);
        this._todayDetail.text = `${this._money(metrics.delta_1h)} last hour`;
        this._rate.text = `${this._money(metrics.rate_per_hour)}/h`;
        this._rateDetail.text = `${this._money(metrics.average_per_day)}/day avg`;
        this._projection.text = this._money(metrics.projected_at_reset);
        this._projectionDetail.text = 'at next reset';
        this._subtitle.text = `${current.plan || 'Copilot'}  ·  ${this._resetText(current.reset_at)}`;

        const daily = Array.isArray(payload.daily) ? payload.daily.slice(-7) : [];
        const maximum = Math.max(1, ...daily.map(day => Number(day.credits) || 0));
        const total = daily.reduce((sum, day) => sum + (Number(day.credits) || 0), 0);
        const hasTrend = daily.filter(day => (Number(day.credits) || 0) > 0).length >= 2;
        this._pulseTotal.text = this._money(total);
        this._chart.visible = hasTrend;
        this._chartLabels.visible = hasTrend;
        this._pulseEmpty.visible = !hasTrend;
        for (let index = 0; index < 7; index++) {
            const day = daily[index] || {};
            const credits = Number(day.credits) || 0;
            this._dailyBars[index].height = credits > 0
                ? Math.round(6 + (credits / maximum) * 44)
                : 2;
            this._dailyLabels[index].text = index === daily.length - 1
                ? 'Today'
                : this._shortDate(day.date);
            if (index === daily.length - 1)
                this._dailyBars[index].add_style_class_name('credit-pulse-chart-bar-current');
            else
                this._dailyBars[index].remove_style_class_name('credit-pulse-chart-bar-current');
        }

        const entitlement = Number(current.entitlement || 0);
        const remaining = Number(current.remaining || 0);
        if (entitlement > 0) {
            this._allowanceText.text = `${this._money(used)} / ${this._money(entitlement)}`;
            this._remaining.text = `${this._money(remaining)} remaining`;
            this._progressTrack.visible = true;
            const fraction = Math.max(0, Math.min(1, used / entitlement));
            this._progress.width = Math.round(350 * fraction);
        } else {
            this._allowanceText.text = 'Unavailable';
            this._remaining.text = 'GitHub did not report a monthly cap for this plan.';
            this._progressTrack.visible = false;
            this._progress.width = 0;
        }

        if (payload.status === 'error')
            this._showError(payload.error || 'GitHub API error');
        else {
            this._error.visible = false;
            this._status.text = '● Live';
            this._status.remove_style_class_name('credit-pulse-status-error');
        }
    }

    _showError(message) {
        this._status.text = '● Cached';
        this._status.add_style_class_name('credit-pulse-status-error');
        this._error.text = String(message);
        this._error.visible = true;
    }

    _number(value) {
        const parsed = Number(value);
        if (!Number.isFinite(parsed))
            return '—';
        return parsed.toLocaleString('en-US', {maximumFractionDigits: 1});
    }

    _money(value, signed = false) {
        if (value === null || value === undefined)
            return '—';
        const parsed = Number(value) / 100.0;
        if (!Number.isFinite(parsed))
            return '—';
        const sign = signed && parsed > 0 ? '+' : '';
        return `${sign}$${parsed.toFixed(2)}`;
    }

    _shortDate(value) {
        const match = String(value || '').match(/^\d{4}-(\d{2})-(\d{2})$/);
        if (!match)
            return '·';
        return `${Number(match[1])}/${Number(match[2])}`;
    }

    _resetText(epoch) {
        const reset = Number(epoch || 0);
        if (!reset)
            return 'No reset reported';
        const days = Math.max(0, Math.ceil((reset * 1000 - Date.now()) / 86400000));
        return days === 1 ? 'Resets tomorrow' : `Resets in ${days} days`;
    }

    destroy() {
        this._cancellable.cancel();
        if (this._pollSource)
            GLib.source_remove(this._pollSource);
        this._cancelClose();
        super.destroy();
    }
});

class Extension {
    enable() {
        this._indicator = new CreditIndicator();
        Main.panel.addToStatusArea('gh-ai-credit-pulse', this._indicator, 1, 'right');
    }

    disable() {
        if (this._indicator) {
            this._indicator.destroy();
            this._indicator = null;
        }
    }
}

function init() {
    return new Extension();
}
