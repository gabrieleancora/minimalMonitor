'use strict';
document.getElementById('login').addEventListener('submit', async (event) => {
  event.preventDefault();
  const form = event.currentTarget;
  const output = document.getElementById('result');
  const button = form.querySelector('button');
  button.disabled = true;
  try {
    const response = await fetch('/auth/login', {
      method: 'POST', credentials: 'same-origin',
      headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': form.elements.csrf.value },
      body: JSON.stringify({ username: form.elements.username.value, password: form.elements.password.value })
    });
    form.elements.password.value = '';
    if (response.ok) location.assign('/');
    else output.textContent = 'Sign-in failed. Try again later or reload the page.';
  } catch (_) {
    output.textContent = 'Connection failed. Try again later.';
  } finally { button.disabled = false; }
});
