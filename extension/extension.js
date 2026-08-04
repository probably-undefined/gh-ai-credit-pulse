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
            text: 'Loading',
            y_align: Clutter.ActorAlign.CENTER,
            style_class: 'credit-pulse-status',
        });
        header.add_child(this._status);
        dashboard.add_child(header);

        const hero = new St.BoxLayout({vertical: true, style_class: 'credit-pulse-hero'});
        hero.add_child(new St.Label({
            text: 'COST USED',
            style_class: 'credit-pulse-kicker',
        }));
        this._used = new St.Label({text: '$—', style_class: 'credit-pulse-hero-value'});
        this._usedDetail = new St.Label({text: '— AIC', style_class: 'credit-pulse-detail'});
        hero.add_child(this._used);
        hero.add_child(this._usedDetail);
        dashboard.add_child(hero);

        const metrics = new St.BoxLayout({style_class: 'credit-pulse-metrics'});
        [
            ['TODAY', '_today', '_todayDetail'],
            ['CURRENT RATE', '_rate', '_rateDetail'],
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

        const refreshItem = new PopupMenu.PopupMenuItem('Refresh now');
        refreshItem.connect('activate', () => this._refresh(true));
        this.menu.addMenuItem(refreshItem);

        const dashboardItem = new PopupMenu.PopupMenuItem('Open full dashboard');
        dashboardItem.connect('activate', () => {
            const launcher = GLib.build_filenamev([GLib.get_home_dir(), '.local', 'bin', 'gh-ai-credit-pulse']);
            try {
                Gio.Subprocess.new([launcher], Gio.SubprocessFlags.NONE);
            } catch (error) {
                this._showError(`Could not open dashboard: ${error.message}`);
            }
        });
        this.menu.addMenuItem(dashboardItem);

        const updateItem = new PopupMenu.PopupMenuItem('Install latest update');
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
        this._status.text = 'Refreshing…';

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
        const used = Number(current.credits_used || 0);

        this._panelLabel.text = this._money(used);
        this._used.text = this._money(used);
        this._usedDetail.text = `${this._number(used)} AIC`;
        this._today.text = this._money(metrics.delta_today, true);
        this._todayDetail.text = `${this._money(metrics.delta_1h)} last hour`;
        this._rate.text = `${this._money(metrics.rate_per_hour)}/h`;
        this._rateDetail.text = `${this._money(metrics.average_per_day)}/day avg`;
        this._projection.text = this._money(metrics.projected_at_reset);
        this._projectionDetail.text = 'at next reset';
        this._subtitle.text = `${current.plan || 'Copilot'}  ·  ${this._resetText(current.reset_at)}`;

        const entitlement = Number(current.entitlement || 0);
        const remaining = Number(current.remaining || 0);
        if (entitlement > 0) {
            this._allowanceText.text = `${this._money(used)} / ${this._money(entitlement)}`;
            this._remaining.text = `${this._money(remaining)} remaining`;
            const fraction = Math.max(0, Math.min(1, used / entitlement));
            this._progress.width = Math.round(310 * fraction);
        } else {
            this._allowanceText.text = 'Not reported';
            this._remaining.text = '';
            this._progress.width = 0;
        }

        if (payload.status === 'error')
            this._showError(payload.error || 'GitHub API error');
        else {
            this._error.visible = false;
            this._status.text = 'Live · 30s';
            this._status.remove_style_class_name('credit-pulse-status-error');
        }
    }

    _showError(message) {
        this._status.text = 'Cached';
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
        const parsed = Number(value) / 100.0;
        if (!Number.isFinite(parsed))
            return '—';
        const sign = signed && parsed > 0 ? '+' : '';
        return `${sign}$${parsed.toFixed(2)}`;
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
