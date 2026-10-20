'use strict';
document.getElementById('show-password').addEventListener('change', event => {
  document.querySelector('[name=password]').type = event.target.checked ? 'text' : 'password';
});
document.getElementById('login').addEventListener('submit', async event => {
  event.preventDefault();
  const form=event.currentTarget, output=document.getElementById('result'), button=form.querySelector('[type=submit]');
  output.textContent='Signing in…';button.disabled=true;
  const controller=new AbortController(), timeout=setTimeout(()=>controller.abort(),6500);
  try {
    const response=await fetch('/auth/login',{method:'POST',credentials:'same-origin',headers:{'Content-Type':'application/json','X-CSRF-Token':form.elements.csrf.value},body:JSON.stringify({username:form.elements.username.value,password:form.elements.password.value}),signal:controller.signal});
    if(response.ok)location.assign('/');
    else output.textContent=response.status===429?`Too many attempts. Wait ${response.headers.get('Retry-After')||'10'} seconds before trying again.`:response.status===403?'Sign-in form expired. Reload this page and try again.':response.status===503?'Sign-in service is busy. Try again shortly.':'Sign-in failed. Check your username and password.';
  } catch (_) {output.textContent='Connection interrupted. Try again shortly.';}
  finally {clearTimeout(timeout);form.elements.password.value='';button.disabled=false;}
});
