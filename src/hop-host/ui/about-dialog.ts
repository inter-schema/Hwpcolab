import { AboutDialog as UpstreamAboutDialog } from '@/upstream/ui';

export class AboutDialog extends UpstreamAboutDialog {
  protected override createBody(): HTMLElement {
    const body = super.createBody();
    const version = body.querySelector('.about-version');

    const hopVersion = document.createElement('div');
    hopVersion.className = 'about-hop-version';
    hopVersion.textContent = `HOP ${__HOP_VERSION__}`;

    const license = document.createElement('div');
    license.className = 'about-license';
    license.style.fontSize = 'var(--font-size-sm, 12px)';
    license.style.color = 'var(--color-text-placeholder, #6b7280)';
    license.style.marginTop = '4px';
    license.textContent = 'License: GNU General Public License v3.0 (GPL-3.0)';

    if (version?.parentNode) {
      version.parentNode.insertBefore(hopVersion, version.nextSibling);
      hopVersion.parentNode?.insertBefore(license, hopVersion.nextSibling);
    } else {
      body.appendChild(hopVersion);
      body.appendChild(license);
    }

    return body;
  }
}
