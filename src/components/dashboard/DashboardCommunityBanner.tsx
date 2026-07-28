/**
 * DashboardCommunityBanner — Slack community callout for the dashboard sidebar.
 *
 * @module components/DashboardCommunityBanner
 */

import { openUrl } from '@tauri-apps/plugin-opener';
import { EyeOffIcon, SlackIcon } from '../icons';
import { Button } from '../primitives/Button';
import { SLACK_INVITE_URL } from '../../lib/links';

interface DashboardCommunityBannerProps {
  onHide: () => void;
}

/**
 * Renders the dashboard community banner with join and hide actions.
 * @param props - Banner dismissal callback.
 */
export function DashboardCommunityBanner({ onHide }: DashboardCommunityBannerProps) {
  return (
    <div className="slack-cta" data-education-id="slack-cta">
      <div className="slack-cta-content">
        <SlackIcon />
        <span>
          <strong>Join the Slack</strong> — suggest features, share what you're building, and shape
          the future of how we build for the web.
        </span>
      </div>
      <Button
        variant="secondary"
        size="sm"
        className="slack-cta-join"
        onClick={() => void openUrl(SLACK_INVITE_URL)}
      >
        Join Slack
      </Button>
      <Button
        variant="ghost"
        size="sm"
        className="slack-cta-hide"
        onClick={onHide}
        title="Hide"
        aria-label="Hide community banner"
      >
        <EyeOffIcon size={14} />
      </Button>
    </div>
  );
}
