import { VerificationResult, UIState, Scenario } from './types.js';

/**
 * Verify if the scenario objective has been achieved or failed
 */
export async function verifyObjective(uiState: UIState, scenario: Scenario): Promise<VerificationResult> {
  const allText = [
    uiState.url,
    ...uiState.visibleText,
    ...uiState.interactables.map((el) => el.label),
  ].join(' ').toLowerCase();

  // Check URL-based fail conditions first (highest priority)
  if (scenario.fail_url_contains) {
    for (const urlPattern of scenario.fail_url_contains) {
      if (uiState.url.toLowerCase().includes(urlPattern.toLowerCase())) {
        return {
          status: 'failure',
          reason: `URL contains fail pattern: ${urlPattern} (current URL: ${uiState.url})`,
          signals_found: [urlPattern],
        };
      }
    }
  }

  // Check for text-based fail signals
  const failSignalsFound: string[] = [];
  for (const signal of scenario.fail_signals) {
    if (allText.includes(signal.toLowerCase())) {
      failSignalsFound.push(signal);
    }
  }

  if (failSignalsFound.length > 0) {
    return {
      status: 'failure',
      reason: `Fail signals detected: ${failSignalsFound.join(', ')}`,
      signals_found: failSignalsFound,
    };
  }

  // Check URL-based success conditions
  const urlSuccessMatches: string[] = [];
  if (scenario.success_url_contains) {
    for (const urlPattern of scenario.success_url_contains) {
      if (uiState.url.toLowerCase().includes(urlPattern.toLowerCase())) {
        urlSuccessMatches.push(urlPattern);
      }
    }
  }

  // Check text-based success signals
  const successSignalsFound: string[] = [];
  for (const signal of scenario.success_signals) {
    if (allText.includes(signal.toLowerCase())) {
      successSignalsFound.push(signal);
    }
  }

  // Combine URL and text success signals
  const allSuccessFound = [...urlSuccessMatches, ...successSignalsFound];

  // Calculate total expected success conditions
  const totalSuccessConditions =
    scenario.success_signals.length +
    (scenario.success_url_contains?.length || 0);

  // Check if all success conditions are met
  if (allSuccessFound.length === totalSuccessConditions && totalSuccessConditions > 0) {
    return {
      status: 'success',
      reason: `All success conditions met: ${allSuccessFound.join(', ')}`,
      signals_found: allSuccessFound,
    };
  }

  // Some success conditions met, but not all
  if (allSuccessFound.length > 0) {
    const missing: string[] = [];

    if (scenario.success_url_contains) {
      for (const urlPattern of scenario.success_url_contains) {
        if (!urlSuccessMatches.includes(urlPattern)) {
          missing.push(`URL should contain: ${urlPattern}`);
        }
      }
    }

    for (const signal of scenario.success_signals) {
      if (!successSignalsFound.includes(signal)) {
        missing.push(signal);
      }
    }

    return {
      status: 'in_progress',
      reason: `Partial success: found ${allSuccessFound.join(', ')}. Still missing: ${missing.join(', ')}`,
      signals_found: allSuccessFound,
    };
  }

  // No success conditions met yet
  return {
    status: 'in_progress',
    reason: 'No success or fail conditions detected yet',
    signals_found: [],
  };
}
