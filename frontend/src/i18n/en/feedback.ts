// The feedback dialog. Reports themselves stay in English: they go to the public tracker.
import { m, type Source } from '../source';

export const FEEDBACK = {
  'feedback.title': m('Send feedback', 'Dialog heading.'),
  'feedback.lede': m('Help make TennoWorth more useful.', 'Dialog text.'),
  'feedback.ask': m('What would you like to share?', 'Question.'),
  'feedback.bug': m('Report a bug', 'Choice.'),
  'feedback.bugHelp': m('Something broke or didn’t work as expected.', 'Choice help.'),
  'feedback.idea': m('Suggest an improvement', 'Choice.'),
  'feedback.ideaHelp': m('Tell us what would make your next trade easier.', 'Choice help.'),
  'feedback.include': m('Include app-state snapshot with bug report', 'Checkbox.'),
  'feedback.contents': m('Version, operating system, current screen, scan, sign-in and update status, error categories, theme, and window size. No account identifiers, credentials, inventory contents, file paths, or game memory.', 'What the snapshot holds.'),
  'feedback.included': m('Included in this report', 'Heading.'),
  'feedback.categoriesOnly': m('Only these categories are sent, not the error text.', 'Note.'),
  'feedback.review': m('Review app-state snapshot', 'Disclosure.'),
  'feedback.refresh': m('Refresh snapshot', 'Button.'),
  'feedback.download': m('Download diagnostics', 'Button.'),
  'feedback.reading': m('Reading app version…', 'Status.'),
  'feedback.tooLarge': m('This snapshot is too large to prefill. Download diagnostics, then attach the file to your GitHub report.', 'Note.'),
  'feedback.downloadFailed': m('The download could not start. Copy the reviewed snapshot into the diagnostics field on GitHub.', 'Error.'),
  'feedback.github': m('Opens GitHub · Account required · Reports are public.', 'Note. Reports are written in English on GitHub; any language is welcome.'),
  'feedback.noBrowser': m('Couldn’t open your browser. Copy this link into your browser to continue.', 'Error.'),
  'feedback.link': m('GitHub issue link', 'Field label.'),
  'feedback.close': m('Close', 'Button.'),
} satisfies Record<string, Source>;
