# Getting started

Buzón is a webmail: you read and write your email from the browser, on your computer or phone. It runs on your own server, so your email doesn't go through anyone else.

## Signing in for the first time

There are two ways to get into Buzón:

- **You installed it.** When you open Buzón's address, the **setup wizard** appears. It asks for an administrator username and password, the language and time zone, how you'd like it to look and, optionally, your first email account.
- **You were invited.** You get an invitation link that **expires after 72 hours**. Open it to choose your Buzón username and password.

::: tip Your Buzón user is not your email
The Buzón username and password only get you into Buzón. Your email account passwords are separate and stored encrypted.
:::

![Buzón in Focus mode](/img/en/inicio.png)

## Adding your email account

Go to **Settings › Email accounts**.

- **Gmail or Outlook:** if the administrator has enabled it, click **Connect with Google** or **Connect with Microsoft** and grant access on their page. No password needed.
- **Any other email:** fill in your address, the IMAP server (to receive), the SMTP server (to send), the username and the password. Buzón suggests servers from your address; if they're wrong, your provider will tell you the right ones.

Buzón checks that it can sign in before saving the account. Then it starts downloading your email: the most recent first, then the rest, little by little.

You can add as many accounts as you like and see them **together** (unified view) or **one at a time**.

## Two-step verification

So nobody can get in with just your password, turn on two-step verification in **Settings › Security**. Scan the QR code with an authenticator app (Aegis, Google Authenticator, 1Password…) and type the 6-digit code. From then on Buzón will ask for it when you sign in.

## Using it as an app

On your phone, open Buzón in the browser and choose **Add to Home Screen**. It opens full screen, like any other app. On a computer, Chrome and Edge offer **Install** in the address bar.

<img src="/img/en/movil.png" alt="Buzón on a phone" width="300">

## Classic or Focus

Buzón has two work modes and you can switch at any time in **Settings › Appearance**. Switching doesn't delete anything.

- **Focus** (default): screens people who email you for the first time and sorts your mail into Main, News and Receipts. See [Focus mode](./focus).
- **Classic:** the usual folders: Inbox, Sent, Drafts, Archive, Spam and Trash.
