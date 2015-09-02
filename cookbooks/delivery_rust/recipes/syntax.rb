include_recipe 'chef-sugar::default'

execute "cargo clean" do
  cwd node['delivery_builder']['repo']
end

execute "git config --global user.email \"delivery@chef.com\"" do
  cwd node['delivery_builder']['repo']
end

execute "git config --global user.name \"Delivery\"" do
  cwd node['delivery_builder']['repo']
end

execute "cargo build" do
  if Chef::VERSION !~ /^12/
    environment({
      'RUST_TEST_TASKS' => "1"
    })
  end
  if windows?
    ruby_version = node['omnibus']['ruby_version']
    environment({
      'PATH' => "C:/rubies/#{ruby_version}/bin;C:/rubies/#{ruby_version}/mingw/bin;C:/Program Files (x86)/Git/Cmd;C:/Program Files (x86)/Git/libexec/git-core;C:/wix;C:/7-zip;C:/Program Files (x86)/Windows Kits/8.1/bin/x64;C:/Windows/system32;C:/Windows;C:/Windows/System32/Wbem;C:/Program Files/OpenSSH/bin;C:/opscode/chef/bin/;C:/opscode/chefdk/bin/;C:/tools/mingw64/bin;C:/Program Files/Rust nightly 1.4/bin;C:/chef/delivery-cli/bin;C:/chef/delivery-cli/bin",
      'HOME' => ENV['USERPROFILE'],
      'C_INCLUDE_PATH' => 'C:/OpenSSL-Win64/include;C:/tools/mingw64/x86_64-w64-mingw32/include',
      'OPENSSL_INCLUDE_DIR' => 'C:/OpenSSL-Win64/include',
      'OPENSSL_LIB_DIR' => 'C:/OpenSSL-Win64',
      'LD_LIBRARY_PATH' => 'C:/OpenSSL-Win64',
      'SSL_CERT_FILE' => "C:/rubies/#{ruby_version}/ssl/certs/cacert.pem'
    })
  end
  cwd node['delivery_builder']['repo']
end
