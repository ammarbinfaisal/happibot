# Nginx (HTTP Only) + Certbot

1. Copy the site config:

```sh
sudo cp deploy/nginx/happiiiiibot.ammarfaisal.me.conf /etc/nginx/sites-available/happiiiiibot.ammarfaisal.me.conf
sudo ln -sf /etc/nginx/sites-available/happiiiiibot.ammarfaisal.me.conf /etc/nginx/sites-enabled/happiiiiibot.ammarfaisal.me.conf
```

2. Create the webroot for ACME challenges:

```sh
sudo mkdir -p /var/www/certbot/.well-known/acme-challenge
sudo chown -R www-data:www-data /var/www/certbot
```

3. Reload nginx:

```sh
sudo nginx -t && sudo systemctl reload nginx
```

4. Get cert (webroot method):

```sh
sudo certbot certonly --webroot -w /var/www/certbot -d happiiiiibot.ammarfaisal.me
```

After cert issuance, add an HTTPS `server { listen 443 ssl; ... }` block (or use `certbot --nginx`).

