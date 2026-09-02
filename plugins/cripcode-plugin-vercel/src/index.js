/**
 * CripCode Vercel plugin.
 *
 * This is an independent implementation of the public CripCode plugin
 * contract. Vercel operations run through the host shell context and the
 * installed Vercel CLI; no Ship Studio service or registry is required.
 */

const React = window.__SHIPSTUDIO_REACT__;
const { createElement, useEffect, useState } = React;

function getContext() {
  const contextRef = window.__SHIPSTUDIO_PLUGIN_CONTEXT_REF__;
  if (contextRef && React.useContext) {
    const context = React.useContext(contextRef);
    if (context) return context;
  }

  const context = window.__SHIPSTUDIO_PLUGIN_CONTEXT__;
  if (context) return context;
  throw new Error('CripCode plugin context is unavailable');
}

const isWindows =
  typeof navigator !== 'undefined' &&
  (/Windows/i.test(navigator.userAgent || '') || /Win/i.test(navigator.platform || ''));

function quoteForCmd(value) {
  if (value === '' || /[\s"&|<>^()%]/.test(value)) {
    return `"${value.replace(/"/g, '""')}"`;
  }
  return value;
}

function execTool(shell, command, args, options) {
  if (isWindows) {
    const commandLine = [command, ...args].map(quoteForCmd).join(' ');
    return shell.exec('cmd', ['/c', commandLine], options);
  }
  return shell.exec(command, args, options);
}

function errorMessage(error) {
  return error instanceof Error ? error.message : String(error);
}

function VercelToolbar() {
  const context = getContext();
  const [state, setState] = useState('checking');

  const checkStatus = async () => {
    try {
      const version = await execTool(context.shell, 'vercel', ['--version'], { timeout: 15 });
      if (version.exit_code !== 0) {
        setState('install');
        return;
      }

      const account = await execTool(context.shell, 'vercel', ['whoami'], { timeout: 30 });
      setState(account.exit_code === 0 ? 'ready' : 'connect');
    } catch {
      setState('install');
    }
  };

  useEffect(() => {
    void checkStatus();
  }, [context.project?.path]);

  const runInstall = async () => {
    setState('installing');
    try {
      const result = await execTool(context.shell, 'npm', ['install', '-g', 'vercel'], {
        timeout: 600,
      });
      if (result.exit_code !== 0)
        throw new Error(result.stderr || 'Vercel CLI installation failed');
      context.actions.showToast('Vercel CLI installed', 'success');
      await checkStatus();
    } catch (error) {
      setState('install');
      context.actions.showToast(`Vercel CLI installation failed: ${errorMessage(error)}`, 'error');
    }
  };

  const runLogin = async () => {
    setState('connecting');
    try {
      await context.actions.openTerminal('vercel', ['login'], { title: 'Vercel Account' });
      await checkStatus();
    } catch (error) {
      setState('connect');
      context.actions.showToast(`Vercel login failed: ${errorMessage(error)}`, 'error');
    }
  };

  const runDeploy = async () => {
    setState('deploying');
    try {
      const result = await execTool(context.shell, 'vercel', ['--prod', '--yes'], { timeout: 600 });
      if (result.exit_code !== 0) throw new Error(result.stderr || 'Vercel deployment failed');
      context.actions.showToast('Deployed to Vercel', 'success');
      await checkStatus();
    } catch (error) {
      setState('ready');
      context.actions.showToast(`Vercel deployment failed: ${errorMessage(error)}`, 'error');
    }
  };

  const actions = {
    checking: { label: 'Checking Vercel...', disabled: true },
    install: { label: 'Install Vercel', onClick: runInstall },
    installing: { label: 'Installing Vercel...', disabled: true },
    connect: { label: 'Connect Vercel', onClick: runLogin },
    connecting: { label: 'Connecting Vercel...', disabled: true },
    ready: { label: 'Deploy to Vercel', onClick: runDeploy },
    deploying: { label: 'Deploying to Vercel...', disabled: true },
  };
  const action = actions[state] || actions.checking;

  return createElement(
    'button',
    {
      type: 'button',
      className: 'toolbar-icon-btn cripcode-vercel-button',
      title: action.label,
      disabled: action.disabled,
      onClick: action.onClick,
    },
    action.label
  );
}

export const name = 'Vercel';
export const slots = { toolbar: VercelToolbar };
