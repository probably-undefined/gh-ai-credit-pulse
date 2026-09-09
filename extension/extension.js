/* exported init */

const {Clutter, Gio, GLib, GObject, Pango, St} = imports.gi;
const Main = imports.ui.main;
const PanelMenu = imports.ui.panelMenu;
const PopupMenu = imports.ui.popupMenu;
const ExtensionUtils = imports.misc.extensionUtils;

const Me = ExtensionUtils.getCurrentExtension();
const POLL_SECONDS = 30;
const CHART_DAYS = 7;
// Heights are expressed in logical pixels; scale them like the CSS dimensions.
const CHART_HEIGHT = 64;

function finiteNumber(value) {
    if (value === null || value === undefined || value === '')
        return null;
    const number = Number(value);
    return Number.isFinite(number) ? number : null;
}

function dateKey(date) {
    return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`;
}

function chartDays(daily, now = new Date()) {
    const byDate = new Map((Array.isArray(daily) ? daily : [])
        .filter(day => day && typeof day.date === 'string')
        .map(day => [day.date, day]));
    return Array.from({length: CHART_DAYS}, (_, index) => {
        // Calendar arithmetic, not 24-hour subtraction, also works across DST.
        const date = new Date(now.getFullYear(), now.getMonth(), now.getDate() - CHART_DAYS + 1 + index);
        const key = dateKey(date);
        const credits = finiteNumber((byDate.get(key) || {}).credits);
        return {
            date: key,
            label: date.toLocaleDateString('en-US', {weekday: 'short'}),
            shortDate: date.toLocaleDateString('en-US', {month: 'short', day: 'numeric'}),
            credits: credits === null ? null : Math.max(0, credits),
            today: key === dateKey(now),
            weekend: date.getDay() === 0 || date.getDay() === 6,
        };
    });
}

const CreditIndicator = GObject.registerClass(
class CreditIndicator extends PanelMenu.Button {
    _init() {
        super._init(0.0, 'GitHub Copilot Usage', false);

        this._pollSource = 0;
        this._closeSource = 0;
        this._refreshing = false;
        this._cancellable = new Gio.Cancellable();
        this._collector = GLib.getenv('GH_AI_CREDIT_PULSE_COLLECTOR') ||
            GLib.build_filenamev([
                GLib.get_user_data_dir(),
                'gh-ai-credit-pulse',
                'gh-ai-credit-pulse-collector',
            ]);
        this.menu.box.add_style_class_name('credit-pulse-menu-content');
        this.menu.box.set_style(
            'padding: 0; background-color: transparent; border: none; box-shadow: none;'
        );
        this.menu.actor.set_style(
            '-arrow-background-color: transparent; -arrow-border-color: transparent; ' +
            '-arrow-border-width: 0px; -arrow-base: 0px; -arrow-rise: 0px;'
        );

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
            text: 'Copilot Usage',
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
            text: '100 AIC = $1.00',
            style_class: 'credit-pulse-conversion',
        }));
        hero.add_child(heroHeader);
        this._used = new St.Label({text: '$—', style_class: 'credit-pulse-hero-value'});
        this._usedDetail = new St.Label({text: '— AIC', style_class: 'credit-pulse-detail'});
        hero.add_child(this._used);
        hero.add_child(this._usedDetail);
        dashboard.add_child(hero);

        const metrics = new St.BoxLayout({style_class: 'credit-pulse-metrics'});
        metrics.get_layout_manager().homogeneous = true;
        [
            ['TODAY', '_today', '_todayDetail'],
            ['6-HOUR RATE', '_rate', '_rateDetail'],
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

        const forecast = new St.BoxLayout({style_class: 'credit-pulse-forecast'});
        const forecastLabels = new St.BoxLayout({vertical: true, x_expand: true});
        forecastLabels.add_child(new St.Label({text: 'PROJECTED AT RESET', style_class: 'credit-pulse-kicker'}));
        this._projectionDetail = new St.Label({text: 'Mon–Fri · 06:00–19:00', style_class: 'credit-pulse-detail'});
        forecastLabels.add_child(this._projectionDetail);
        forecast.add_child(forecastLabels);
        this._projection = new St.Label({text: '—', y_align: Clutter.ActorAlign.CENTER, style_class: 'credit-pulse-card-value'});
        forecast.add_child(this._projection);
        dashboard.add_child(forecast);

        const pulse = new St.BoxLayout({vertical: true, style_class: 'credit-pulse-pulse'});
        const pulseHeader = new St.BoxLayout();
        pulseHeader.add_child(new St.Label({
            text: 'LAST 7 DAYS',
            x_expand: true,
            style_class: 'credit-pulse-kicker credit-pulse-kicker-violet',
        }));
        this._pulseTotal = new St.Label({text: '$—', style_class: 'credit-pulse-pulse-total'});
        pulseHeader.add_child(this._pulseTotal);
        pulse.add_child(pulseHeader);
        this._pulseRange = new St.Label({text: 'Including today · local time', style_class: 'credit-pulse-detail'});
        pulse.add_child(this._pulseRange);
        const chart = new St.BoxLayout({style_class: 'credit-pulse-chart'});
        chart.get_layout_manager().homogeneous = true;
        this._chart = chart;
        this._dailyBars = [];
        this._dailyLabels = [];
        this._dailyValues = [];
        this._dailySlots = [];
        for (let index = 0; index < CHART_DAYS; index++) {
            // Values, bars and labels share a column so their centers always align.
            const column = new St.BoxLayout({vertical: true, x_expand: true, style_class: 'credit-pulse-chart-column'});
            const value = new St.Label({text: '—', style_class: 'credit-pulse-chart-value'});
            column.add_child(value);
            const slot = new St.Bin({style_class: 'credit-pulse-chart-slot'});
            const bar = new St.Widget({
                x_align: Clutter.ActorAlign.CENTER,
                y_align: Clutter.ActorAlign.END,
                style_class: 'credit-pulse-chart-bar credit-pulse-chart-bar-weekday',
            });
            slot.set_child(bar);
            column.add_child(slot);
            const label = new St.Label({text: '·', style_class: 'credit-pulse-chart-label'});
            column.add_child(label);
            chart.add_child(column);
            this._dailyBars.push(bar);
            this._dailyLabels.push(label);
            this._dailyValues.push(value);
            this._dailySlots.push(slot);
        }
        pulse.add_child(chart);
        const legend = new St.BoxLayout({style_class: 'credit-pulse-legend'});
        [['weekday', 'Weekday'], ['weekend', 'Weekend'], ['current', 'Today']].forEach(([kind, title]) => {
            const item = new St.BoxLayout({style_class: 'credit-pulse-legend-item'});
            item.add_child(new St.Widget({style_class: `credit-pulse-legend-swatch credit-pulse-chart-bar-${kind}`}));
            item.add_child(new St.Label({text: title, y_align: Clutter.ActorAlign.CENTER, style_class: 'credit-pulse-legend-label'}));
            legend.add_child(item);
        });
        this._chartLegend = legend;
        pulse.add_child(legend);
        this._pulseEmpty = new St.Label({
            text: 'Waiting for the first usage sample.',
            visible: false,
            style_class: 'credit-pulse-empty',
        });
        pulse.add_child(this._pulseEmpty);
        this._pulseNote = new St.Label({text: 'Recorded changes · includes today', style_class: 'credit-pulse-detail'});
        pulse.add_child(this._pulseNote);
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
        this._progress = new St.Widget({x_align: Clutter.ActorAlign.START, style_class: 'credit-pulse-progress'});
        this._progressFraction = 0;
        this._progressTrack.connect('notify::allocation', () => this._updateProgress());
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
        [this._subtitle, this._remaining, this._error, this._projectionDetail, this._pulseNote].forEach(label => {
            label.clutter_text.ellipsize = Pango.EllipsizeMode.NONE;
            label.clutter_text.line_wrap = true;
            label.clutter_text.line_wrap_mode = Pango.WrapMode.WORD_CHAR;
        });

        this.menu.addMenuItem(contentItem);
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
                [this._collector, fetch ? 'sample' : 'dashboard', '--window', '24h'],
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
        const used = finiteNumber(current.credits_used);
        const rate = finiteNumber(metrics.rate_per_hour);

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
        this._projectionDetail.text = 'Mon–Fri · 06:00–19:00';
        this._subtitle.text = `${current.plan || 'Copilot'}  ·  ${this._resetText(current.reset_at)}`;

        const daily = chartDays(payload.daily);
        const knownDays = daily.filter(day => day.credits !== null);
        const maximum = Math.max(1, ...knownDays.map(day => day.credits));
        const total = knownDays.reduce((sum, day) => sum + day.credits, 0);
        const hasHistory = knownDays.length > 0;
        const complete = knownDays.length === CHART_DAYS;
        this._pulseTotal.text = hasHistory ? `${complete ? '' : '≥ '}${this._money(total)}` : '—';
        this._pulseRange.text = `${daily[0].shortDate} – ${daily[CHART_DAYS - 1].shortDate} · local time`;
        this._chart.visible = hasHistory;
        this._chartLegend.visible = hasHistory;
        this._pulseEmpty.visible = !hasHistory;
        this._pulseNote.visible = hasHistory;
        this._pulseNote.text = payload.sample_count === 1
            ? 'First sample saved; waiting for a change.'
            : complete ? 'Recorded changes · includes today' : 'Partial history · — means no data';
        const scale = St.ThemeContext.get_for_stage(global.stage).scale_factor;
        for (let index = 0; index < CHART_DAYS; index++) {
            const day = daily[index];
            const bar = this._dailyBars[index];
            bar.visible = day.credits !== null;
            // Zero is a baseline; positive heights remain proportional to usage.
            bar.height = Math.round(day.credits > 0 ? Math.max(scale, day.credits / maximum * CHART_HEIGHT * scale) : scale);
            this._dailyValues[index].text = this._money(day.credits);
            this._dailyLabels[index].text = day.today ? 'Today' : day.label;
            this._dailySlots[index].accessible_name = `${day.shortDate}: ${this._money(day.credits)}`;
            this._setBarKind(bar, day.today ? 'current' : day.weekend ? 'weekend' : 'weekday');
            this._dailyLabels[index].set_style_class_name(
                day.today ? 'credit-pulse-chart-label credit-pulse-chart-label-current' : 'credit-pulse-chart-label'
            );
        }

        const entitlement = finiteNumber(current.entitlement);
        const remaining = finiteNumber(current.remaining);
        if (entitlement > 0) {
            this._allowanceText.text = `${this._money(used)} / ${this._money(entitlement)}`;
            this._remaining.text = `${this._money(remaining)} remaining`;
            this._progressTrack.visible = true;
            const fraction = Math.max(0, Math.min(1, (used || 0) / entitlement));
            this._progressFraction = fraction;
            this._updateProgress();
            this._setProgressTone(fraction);
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
            const live = payload.status === 'ok' && payload.fresh === true && used !== null;
            this._status.text = live ? '● Live' : used === null ? '● No data' : '● Cached';
            if (live)
                this._status.remove_style_class_name('credit-pulse-status-stale');
            else
                this._status.add_style_class_name('credit-pulse-status-stale');
        }
    }

    _updateProgress() {
        const box = this._progressTrack.get_theme_node().get_content_box(this._progressTrack.get_allocation_box());
        this._progress.width = Math.round(Math.max(0, box.get_width()) * this._progressFraction);
    }

    _showError(message) {
        this._status.text = '● Cached';
        this._status.add_style_class_name('credit-pulse-status-stale');
        this._error.text = String(message);
        this._error.visible = true;
    }

    _number(value) {
        const parsed = finiteNumber(value);
        if (parsed === null)
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

    _setBarKind(bar, kind) {
        bar.set_style_class_name(`credit-pulse-chart-bar credit-pulse-chart-bar-${kind}`);
    }

    // Mint while comfortably inside the cap, amber from 75%, red from 90%.
    _setProgressTone(fraction) {
        const tone = fraction >= 0.9 ? 'critical' : fraction >= 0.75 ? 'warning' : 'ok';
        this._progress.set_style_class_name(`credit-pulse-progress credit-pulse-progress-${tone}`);
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
