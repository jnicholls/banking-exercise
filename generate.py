account_id = 0

print("type,client,tx,amount")

for txn_id in range(1, 1000000):
	print(f"deposit,{(account_id % 8) + 1},{txn_id},1.2345") 
	account_id += 1
